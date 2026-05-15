use std::sync::LazyLock;

use async_trait::async_trait;
use tokio::sync::Mutex;
use whale_land::{Connection, Error, NewObject, NewObjectId, ObjectId, Packet};
use whale_land::protocol::EventHandler;
use whale_land::protocol::wayland::{
    wl_compositor_v7_request_proxy, wl_registry_v1_event_handler, wl_registry_v1_request_proxy,
    wl_surface_v7_request_proxy,
};
use whale_land::protocol::wlr_layer_shell_unstable_v1::{
    zwlr_layer_shell_v1_v5_layer_u32, zwlr_layer_shell_v1_v5_request_proxy,
};
use tracing::{debug, instrument};


#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct DisplayState {
    pub compositor_oid: Option<ObjectId>,
    pub layer_shell_oid: Option<ObjectId>,
}
static DISPLAY_STATE: LazyLock<Mutex<DisplayState>> = LazyLock::new(|| Mutex::new(DisplayState::default()));


struct RegistryHandler;
impl RegistryHandler {
    async fn surface_creation_rigmarole(&self, connection: &Connection) {
        let (compositor_oid, layer_shell_oid) = {
            let display_state_guard = DISPLAY_STATE.lock().await;
            let Some(comp) = display_state_guard.compositor_oid
                else { return };
            let Some(lsh) = display_state_guard.layer_shell_oid
                else { return };
            (comp, lsh)
        };

        // gimme a surface
        let compositor = wl_compositor_v7_request_proxy::new(
            compositor_oid,
            connection.downgrade(),
        );
        let surface_oid = connection.get_and_increment_next_object_id();
        compositor.send_create_surface(NewObjectId(surface_oid))
            .await.expect("failed to create surface");

        // transform it into a layer surface
        let layer_shell = zwlr_layer_shell_v1_v5_request_proxy::new(
            layer_shell_oid,
            connection.downgrade(),
        );
        let layer_surface_oid = connection.get_and_increment_next_object_id();
        layer_shell.send_get_layer_surface(
            NewObjectId(layer_surface_oid),
            Some(surface_oid),
            None,
            zwlr_layer_shell_v1_v5_layer_u32::OVERLAY.into(),
            "activation",
        ).await.expect("failed to promote surface to layer surface");

        // TODO: add event handler

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
    #[instrument(skip_all)]
    async fn handle_global(
        &self,
        connection: &whale_land::Connection,
        packet: whale_land::Packet,
        name: u32,
        interface: ::std::string::String,
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
        }
    }

    #[instrument(skip_all)]
    async fn handle_global_remove(
        &self,
        connection: &whale_land::Connection,
        packet: whale_land::Packet,
        name: u32,
    ) {
        let _ = (connection, packet);
        debug!("{} is gone", name);
    }
}


async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // connect to Wayland
    let wl = Connection::new_from_env()
        .await.expect("failed to connect to Wayland");

    // make a registry
    let registry_oid = wl.get_and_increment_next_object_id();
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
