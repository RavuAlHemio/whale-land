use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::net::UnixStream;
use tokio::sync::{Mutex, RwLock};
use tracing::debug;

use crate::protocol::wayland::wl_display_v1_request_proxy;
use crate::{Error, ObjectId, Packet};
use crate::protocol::EventHandler;
use crate::socket_fd_ext::SocketFdExt;


const RUNTIME_DIR_VAR: &str = "XDG_RUNTIME_DIR";
const WAYLAND_DISPLAY_VAR: &str = "WAYLAND_DISPLAY";
const DEFAULT_WAYLAND_DISPLAY: &str = "wayland-0";


pub struct Connection {
    inner: Arc<InnerConnection>,
}
impl Connection {
    pub async fn new_from_env() -> Result<Self, Error> {
        let runtime_dir = env::var_os(RUNTIME_DIR_VAR)
            .ok_or_else(|| Error::MissingEnvVar { name: RUNTIME_DIR_VAR.to_owned() })?;
        let wayland_display = env::var_os(WAYLAND_DISPLAY_VAR)
            .unwrap_or_else(|| OsString::from(DEFAULT_WAYLAND_DISPLAY));
        let mut wayland_display_path = PathBuf::from(&runtime_dir);
        wayland_display_path.push(&wayland_display);

        Self::new_from_socket_path(&wayland_display_path).await
    }

    pub async fn new_from_socket_path(path: &Path) -> Result<Self, Error> {
        let socket = UnixStream::connect(path).await?;
        Ok(Self {
            inner: Arc::new(InnerConnection {
                socket,
                send_lock: Mutex::new(()),
                recv_lock: Mutex::new(()),
                next_object_id: AtomicU32::new(2), // 0 is NULL, 1 is always wl_display
                object_id_to_event_handler: RwLock::new(BTreeMap::new()),
            }),
        })
    }

    pub async fn send_packet(&self, packet: &Packet) -> Result<(), Error> {
        let serialized = packet.serialize()?;

        {
            let send_guard = self.inner.send_lock.lock().await;

            // SocketFdExt functions handle WouldBlock for us
            let mut total_sent = self.inner.socket
                .send_with_fds(&serialized, packet.fds()).await?;

            while total_sent < serialized.len() {
                // send more
                let now_sent = self.inner.socket.send(&serialized[total_sent..]).await?;
                total_sent += now_sent;
            }

            drop(send_guard);
        }

        Ok(())
    }

    pub async fn recv_packet(&self) -> Result<Packet, Error> {
        let packet = {
            let recv_guard = self.inner.recv_lock.lock().await;

            // sender ID, size, opcode
            let mut fixed_buf = [0u8; 8];
            let mut fds = Vec::new();

            // SocketFdExt functions handle WouldBlock for us
            let (mut total_received, _fd_count) = self.inner.socket
                .recv_with_fds(&mut fixed_buf, &mut fds).await?;
            while total_received < fixed_buf.len() {
                // receive more
                let (now_received, _now_received_fds) = self.inner.socket
                    .recv_with_fds(&mut fixed_buf[total_received..], &mut fds).await?;
                total_received += now_received;
            }

            let object_id_u32 = u32::from_ne_bytes(fixed_buf[0..4].try_into().unwrap());
            let size_and_opcode = u32::from_ne_bytes(fixed_buf[4..8].try_into().unwrap());
            let packet_size: usize = (size_and_opcode >> 16).try_into().unwrap();
            let opcode: u16 = (size_and_opcode & 0xFF).try_into().unwrap();

            if packet_size < 8 {
                // 8 bytes are the fixed header and thereby the minimum
                return Err(Error::PacketTooShort { actual: packet_size, minimum: 8 });
            }

            let object_id_nz = NonZero::new(object_id_u32)
                .ok_or(Error::ZeroObjectId)?;
            let object_id = ObjectId(object_id_nz);

            // read the payload
            let mut payload = vec![0u8; packet_size - 8];
            (total_received, _) = self.inner.socket
                .recv_with_fds(&mut payload, &mut fds).await?;
            while total_received < payload.len() {
                let (now_received, _) = self.inner.socket
                    .recv_with_fds(&mut payload[total_received..], &mut fds).await?;
                total_received += now_received;
            }

            drop(recv_guard);

            Packet::new_from_existing(
                object_id,
                opcode,
                payload,
                fds,
            )
        };

        Ok(packet)
    }

    pub fn get_and_increment_next_object_id(&self) -> ObjectId {
        loop {
            let new_val = self.inner.next_object_id.fetch_add(1, Ordering::SeqCst);
            if let Some(oid) = ObjectId::new(new_val) {
                return oid;
            }
        }
    }

    pub fn get_display_proxy(&self) -> wl_display_v1_request_proxy {
        wl_display_v1_request_proxy::new(ObjectId::DISPLAY, self.downgrade())
    }

    pub async fn register_handler(&self, object_id: ObjectId, event_handler: Box<dyn EventHandler + Send + Sync>) {
        let mut map_guard = self.inner.object_id_to_event_handler
            .write().await;
        map_guard
            .insert(object_id, event_handler);
    }

    pub async fn drop_handler(&self, object_id: ObjectId) {
        let mut map_guard = self.inner.object_id_to_event_handler
            .write().await;
        map_guard
            .remove(&object_id);
    }

    pub async fn dispatch(&self, packet: Packet) -> Result<(), Error> {
        let map_guard = self.inner.object_id_to_event_handler
            .read().await;
        let event_handler = map_guard
            .get(&packet.object_id());
        match event_handler {
            Some(eh) => eh.handle_event(self, packet).await,
            None => {
                debug!("dropping packet as there is no handler: {:?}", packet);
                Err(Error::NoEventHandler {
                    object_id: packet.object_id(),
                })
            },
        }
    }

    pub fn downgrade(&self) -> WeakConnection {
        WeakConnection { inner: Arc::downgrade(&self.inner) }
    }
}

pub struct WeakConnection {
    inner: Weak<InnerConnection>,
}
impl WeakConnection {
    pub fn upgrade(&self) -> Option<Connection> {
        self.inner.upgrade()
            .map(|i| Connection { inner: i })
    }
}

struct InnerConnection {
    socket: UnixStream,
    send_lock: Mutex<()>,
    recv_lock: Mutex<()>,
    next_object_id: AtomicU32,
    object_id_to_event_handler: RwLock<BTreeMap<ObjectId, Box<dyn EventHandler + Send + Sync>>>,
}
