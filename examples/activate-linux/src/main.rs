mod draw_text;


use std::os::fd::AsRawFd;
use std::sync::LazyLock;

use async_trait::async_trait;
use tokio::sync::Mutex;
use whale_land::shared_memory::SharedMemoryObject;
use whale_land::{Connection, Error, NewObject, NewObjectId, ObjectId, Packet};
use whale_land::protocol::EventHandler;
use whale_land::protocol::wayland::{
    wl_buffer_v1_event_handler, wl_compositor_v7_request_proxy, wl_display_v1_event_handler,
    wl_output_v4_transform_u32, wl_registry_v1_event_handler, wl_registry_v1_request_proxy,
    wl_shm_pool_v2_request_proxy, wl_shm_v2_event_handler, wl_shm_v2_format_u32,
    wl_shm_v2_request_proxy, wl_surface_v7_event_handler, wl_surface_v7_request_proxy,
};
use whale_land::protocol::wlr_layer_shell_unstable_v1::{
    zwlr_layer_shell_v1_v5_layer_u32, zwlr_layer_shell_v1_v5_request_proxy,
    zwlr_layer_surface_v1_v5_anchor_u32, zwlr_layer_surface_v1_v5_event_handler,
    zwlr_layer_surface_v1_v5_request_proxy,
};
use tracing::{debug, error, info, instrument};


#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct DisplayState {
    pub compositor_oid: Option<ObjectId>,
    pub layer_shell_oid: Option<ObjectId>,
    pub shm_oid: Option<ObjectId>,
    pub surface_oid: Option<ObjectId>,
    pub layer_surface_oid: Option<ObjectId>,
    pub preferred_scale: i32,
    pub preferred_transform: wl_output_v4_transform_u32,
    pub width: u32,
    pub height: u32,
}
impl Default for DisplayState {
    fn default() -> Self {
        Self {
            compositor_oid: None,
            layer_shell_oid: None,
            shm_oid: None,
            surface_oid: None,
            layer_surface_oid: None,
            preferred_scale: 1,
            preferred_transform: wl_output_v4_transform_u32::NORMAL,
            width: 0,
            height: 0,
        }
    }
}
static DISPLAY_STATE: LazyLock<Mutex<DisplayState>> = LazyLock::new(|| Mutex::new(DisplayState::default()));


#[derive(Debug)]
struct DisplayHandler;
#[async_trait]
impl EventHandler for DisplayHandler {
    async fn handle_event(&self, connection: &Connection, packet: Packet) -> Result<(), Error> {
        wl_display_v1_event_handler::handle_event(self, connection, packet).await
    }
}
impl wl_display_v1_event_handler for DisplayHandler {
    #[instrument(skip(connection, packet))]
    async fn handle_error(
        &self,
        connection: &Connection,
        packet: Packet,
        object_id: Option<ObjectId>,
        code: u32,
        message: String,
    ) {
        let _ = (connection, packet);
        error!("error received");
    }

    #[instrument(skip(connection, packet))]
    async fn handle_delete_id(
        &self,
        connection: &Connection,
        packet: Packet,
        id: u32,
    ) {
        let _ = (connection, packet);
        debug!("object deleted");
    }
}


#[derive(Debug)]
struct RegistryHandler;
impl RegistryHandler {
    async fn surface_creation_rigmarole(&self, connection: &Connection) {
        let (compositor_oid, layer_shell_oid) = {
            let display_state_guard = DISPLAY_STATE.lock().await;
            let Some(comp) = display_state_guard.compositor_oid
                else { return };
            let Some(lsh) = display_state_guard.layer_shell_oid
                else { return };
            if display_state_guard.shm_oid.is_none() {
                return;
            }
            (comp, lsh)
        };

        // gimme a surface
        let compositor = wl_compositor_v7_request_proxy::new(
            compositor_oid,
            connection.downgrade(),
        );
        let surface_oid = connection.get_and_increment_next_object_id();
        debug!("surface OID is {}", surface_oid.0);
        {
            let mut display_state_guard = DISPLAY_STATE.lock().await;
            display_state_guard.surface_oid = Some(surface_oid);
        }
        compositor.send_create_surface(NewObjectId(surface_oid))
            .await.expect("failed to create surface");

        // transform it into a layer surface
        let layer_shell = zwlr_layer_shell_v1_v5_request_proxy::new(
            layer_shell_oid,
            connection.downgrade(),
        );
        let layer_surface_oid = connection.get_and_increment_next_object_id();
        debug!("layer surface OID is {}", layer_surface_oid.0);
        {
            let mut display_state_guard = DISPLAY_STATE.lock().await;
            display_state_guard.layer_surface_oid = Some(layer_surface_oid);
        }
        layer_shell.send_get_layer_surface(
            NewObjectId(layer_surface_oid),
            Some(surface_oid),
            None,
            zwlr_layer_shell_v1_v5_layer_u32::OVERLAY.into(),
            "activation",
        ).await.expect("failed to promote surface to layer surface");

        connection.register_handler(surface_oid, Box::new(SurfaceHandler))
            .await;
        connection.register_handler(layer_surface_oid, Box::new(LayerSurfaceHandler))
            .await;

        // commit it once while it's empty
        let surface = wl_surface_v7_request_proxy::new(
            surface_oid,
            connection.downgrade(),
        );
        surface.send_commit()
            .await.expect("failed to initially commit surface");
    }
}
#[async_trait]
impl EventHandler for RegistryHandler {
    async fn handle_event(&self, connection: &Connection, packet: Packet) -> Result<(), Error> {
        wl_registry_v1_event_handler::handle_event(self, connection, packet).await
    }
}
impl wl_registry_v1_event_handler for RegistryHandler {
    #[instrument(skip(connection, packet))]
    async fn handle_global(
        &self,
        connection: &Connection,
        packet: Packet,
        name: u32,
        interface: String,
        version: u32,
    ) {
        debug!("{} is {:?} version {}", name, interface, version);

        let registry = wl_registry_v1_request_proxy::new(
            packet.object_id(),
            connection.downgrade(),
        );

        if interface == "wl_compositor" {
            // yeah, I want that one
            let compositor_oid = connection.get_and_increment_next_object_id();
            debug!("compositor OID is {}", compositor_oid.0);
            registry.send_bind(
                name,
                NewObject {
                    object_id: compositor_oid,
                    interface: interface.to_owned(),
                    interface_version: version,
                },
            ).await.expect("failed to bind to compositor");

            let mut display_state_guard = DISPLAY_STATE.lock().await;
            display_state_guard.compositor_oid = Some(compositor_oid);
            drop(display_state_guard);

            self.surface_creation_rigmarole(connection).await;
        } else if interface == "zwlr_layer_shell_v1" {
            // I want this one too
            let layer_shell_oid = connection.get_and_increment_next_object_id();
            debug!("layer shell OID is {}", layer_shell_oid.0);
            registry.send_bind(
                name,
                NewObject {
                    object_id: layer_shell_oid,
                    interface: interface.to_owned(),
                    interface_version: version,
                },
            ).await.expect("failed to bind to layer shell");

            let mut display_state_guard = DISPLAY_STATE.lock().await;
            display_state_guard.layer_shell_oid = Some(layer_shell_oid);
            drop(display_state_guard);

            self.surface_creation_rigmarole(connection).await;
        } else if interface == "wl_shm" {
            // gimme
            let shm_oid = connection.get_and_increment_next_object_id();
            debug!("shm OID is {}", shm_oid.0);
            connection.register_handler(shm_oid, Box::new(SharedMemoryHandler)).await;
            registry.send_bind(
                name,
                NewObject {
                    object_id: shm_oid,
                    interface: interface.to_owned(),
                    interface_version: version,
                },
            ).await.expect("failed to bind to shared memory provider");

            let mut display_state_guard = DISPLAY_STATE.lock().await;
            display_state_guard.shm_oid = Some(shm_oid);
            drop(display_state_guard);

            self.surface_creation_rigmarole(connection).await;
        }
    }

    #[instrument(skip(connection, packet))]
    async fn handle_global_remove(
        &self,
        connection: &Connection,
        packet: Packet,
        name: u32,
    ) {
        let _ = (connection, packet);
        debug!("{} is gone", name);
    }
}


#[derive(Debug)]
struct SurfaceHandler;
#[async_trait]
impl EventHandler for SurfaceHandler {
    async fn handle_event(&self, connection: &Connection, packet: Packet) -> Result<(), Error> {
        wl_surface_v7_event_handler::handle_event(self, connection, packet).await
    }
}
impl wl_surface_v7_event_handler for SurfaceHandler {
    #[instrument(skip(connection, packet))]
    async fn handle_enter(
        &self,
        connection: &Connection,
        packet: Packet,
        output: Option<ObjectId>,
    ) {
        let _ = (connection, packet);
        info!("enter");
    }

    #[instrument(skip(connection, packet))]
    async fn handle_leave(
        &self,
        connection: &Connection,
        packet: Packet,
        output: Option<ObjectId>,
    ) {
        let _ = (connection, packet);
        info!("leave");
    }

    #[instrument(skip(connection, packet))]
    async fn handle_preferred_buffer_scale(
        &self,
        connection: &Connection,
        packet: Packet,
        factor: i32,
    ) {
        let _ = (connection, packet);
        info!("preferred_buffer_scale");
        let mut state_guard = DISPLAY_STATE.lock().await;
        state_guard.preferred_scale = factor;
    }

    #[instrument(skip(connection, packet))]
    async fn handle_preferred_buffer_transform(
        &self,
        connection: &Connection,
        packet: Packet,
        transform: u32,
    ) {
        let _ = (connection, packet);
        info!("preferred_buffer_transform");
        let mut state_guard = DISPLAY_STATE.lock().await;
        state_guard.preferred_transform = transform.into();
    }
}


#[derive(Debug)]
struct LayerSurfaceHandler;
#[async_trait]
impl EventHandler for LayerSurfaceHandler {
    async fn handle_event(&self, connection: &Connection, packet: Packet) -> Result<(), Error> {
        zwlr_layer_surface_v1_v5_event_handler::handle_event(self, connection, packet).await
    }
}
impl zwlr_layer_surface_v1_v5_event_handler for LayerSurfaceHandler {
    #[instrument(skip(connection, packet))]
    async fn handle_configure(
        &self,
        connection: &Connection,
        packet: Packet,
        serial: u32,
        width: u32,
        height: u32,
    ) {
        let _ = packet;
        let mut state_guard = DISPLAY_STATE.lock().await;
        state_guard.width = width;
        state_guard.height = height;

        // acknowledge the configuration
        let layer_surface = zwlr_layer_surface_v1_v5_request_proxy::new(
            state_guard.layer_surface_oid.unwrap(),
            connection.downgrade(),
        );
        layer_surface.send_ack_configure(serial)
            .await.expect("failed to send configuration acknowledgement");

        // calculate the surface data
        let mut text_image = crate::draw_text::draw_text(state_guard.preferred_scale as f32);

        // ensure its size is divisible by our scaling factor
        let preferred_scale_u32 = u32::try_from(state_guard.preferred_scale.abs()).unwrap();
        while text_image.width % preferred_scale_u32 != 0 {
            text_image.width += 1;
        }
        while text_image.height % preferred_scale_u32 != 0 {
            text_image.height += 1;
        }
        let text_image_argb = text_image.to_white_argb_le();

        // set the surface size
        layer_surface.send_set_size(
            text_image.width / preferred_scale_u32,
            text_image.height / preferred_scale_u32,
        )
            .await.expect("failed to set surface size");

        // anchor the surface to the bottom right
        layer_surface.send_set_anchor(
            u32::from(zwlr_layer_surface_v1_v5_anchor_u32::BOTTOM)
            | u32::from(zwlr_layer_surface_v1_v5_anchor_u32::RIGHT)
        )
            .await.expect("failed to reanchor surface");

        // give it a fixed margin from that corner
        layer_surface.send_set_margin(
            0,
            crate::draw_text::RIGHT_MARGIN,
            crate::draw_text::BOTTOM_MARGIN,
            0,
        )
            .await.expect("failed to set surface margin");

        // create an empty region to tell the compositor that we aren't interactive
        let compositor = wl_compositor_v7_request_proxy::new(
            state_guard.compositor_oid.unwrap(),
            connection.downgrade(),
        );
        let empty_region_oid = connection.get_and_increment_next_object_id();
        debug!("empty region OID is {}", empty_region_oid.0);
        compositor.send_create_region(NewObjectId(empty_region_oid))
            .await.expect("failed to create region");

        // create a shared memory segment
        let shared_memory_length: i32 = text_image_argb.len().try_into().unwrap();
        let mut shared_memory = SharedMemoryObject::new_anonymous()
            .expect("failed to create shared memory segment");
        shared_memory.set_length(shared_memory_length.into())
            .expect("failed to set shared memory length");

        // make a shmem pool out of it
        let shm = wl_shm_v2_request_proxy::new(
            state_guard.shm_oid.unwrap(),
            connection.downgrade(),
        );
        let shm_pool_id = connection.get_and_increment_next_object_id();
        debug!("shm pool OID is {}", shm_pool_id.0);
        shm.send_create_pool(
            NewObjectId(shm_pool_id),
            shared_memory.as_raw_fd(),
            shared_memory_length,
        )
            .await.expect("failed to create shared memory pool");

        // take a buffer from the pool
        let shm_pool = wl_shm_pool_v2_request_proxy::new(
            shm_pool_id,
            connection.downgrade(),
        );
        let buffer_oid = connection.get_and_increment_next_object_id();
        debug!("buffer OID is {}", buffer_oid.0);
        connection.register_handler(buffer_oid, Box::new(BufferHandler)).await;
        shm_pool.send_create_buffer(
            NewObjectId(buffer_oid),
            0,
            text_image.width.try_into().unwrap(),
            text_image.height.try_into().unwrap(),
            i32::try_from(text_image.width).unwrap() * 4,
            wl_shm_v2_format_u32::ARGB8888.into(),
        )
            .await.expect("failed to create shared memory buffer");

        // write the image into the buffer
        {
            let shared_mapping = shared_memory.map_read_write(..)
                .expect("failed to map shared memory");
            let shared_mapping_slice = unsafe {
                std::slice::from_raw_parts_mut(
                    shared_mapping.as_mut_ptr() as *mut u8,
                    text_image_argb.len(),
                )
            };
            shared_mapping_slice.copy_from_slice(&text_image_argb);
        }

        // attach the buffer to the surface
        let surface = wl_surface_v7_request_proxy::new(
            state_guard.surface_oid.unwrap(),
            connection.downgrade(),
        );
        surface.send_attach(
            Some(buffer_oid),
            0,
            0,
        )
            .await.expect("failed to attach buffer to surface");

        // set the scaling factor
        surface.send_set_buffer_scale(state_guard.preferred_scale)
            .await.expect("failed to set buffer scaling");

        // assign the empty interaction region
        surface.send_set_input_region(Some(empty_region_oid))
            .await.expect("failed to set input region");

        // commit the surface
        surface.send_commit()
            .await.expect("failed to commit buffer");
    }

    #[instrument(skip(connection, packet))]
    async fn handle_closed(
        &self,
        connection: &Connection,
        packet: Packet,
    ) {
        let _ = (connection, packet);
        info!("closed");
    }
}


#[derive(Debug)]
struct SharedMemoryHandler;
#[async_trait]
impl EventHandler for SharedMemoryHandler {
    async fn handle_event(&self, connection: &Connection, packet: Packet) -> Result<(), Error> {
        wl_shm_v2_event_handler::handle_event(self, connection, packet).await
    }
}
impl wl_shm_v2_event_handler for SharedMemoryHandler {
    #[instrument(skip(connection, packet))]
    async fn handle_format(
        &self,
        connection: &Connection,
        packet: Packet,
        format: u32,
    ) {
        let _ = (connection, packet);
        debug!("supported pixel buffer format: {}", format);
    }
}


#[derive(Debug)]
struct BufferHandler;
#[async_trait]
impl EventHandler for BufferHandler {
    async fn handle_event(&self, connection: &Connection, packet: Packet) -> Result<(), Error> {
        wl_buffer_v1_event_handler::handle_event(self, connection, packet).await
    }
}
impl wl_buffer_v1_event_handler for BufferHandler {
    #[instrument(skip(connection, packet))]
    async fn handle_release(
        &self,
        connection: &Connection,
        packet: Packet,
    ) {
        let _ = (connection, packet);
        info!("buffer released");
    }
}


async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // connect to Wayland
    let wl = Connection::new_from_env()
        .await.expect("failed to connect to Wayland");

    // register a display handler
    wl.register_handler(whale_land::ObjectId::DISPLAY, Box::new(DisplayHandler)).await;

    // make a registry
    let registry_oid = wl.get_and_increment_next_object_id();
    debug!("registry OID is {}", registry_oid.0);
    wl.register_handler(registry_oid, Box::new(RegistryHandler)).await;
    let display = wl.get_display_proxy();
    display.send_get_registry(NewObjectId(registry_oid))
        .await.expect("failed to send request for registry");

    // handle it all
    loop {
        let packet = wl.recv_packet()
            .await.expect("failed to receive Wayland packet");
        debug!("{:?}", packet);
        wl.dispatch(packet)
            .await.expect("failed to dispatch Wayland packet");
    };
}


#[tokio::main]
async fn main() {
    run().await
}
