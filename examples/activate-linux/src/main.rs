use async_trait::async_trait;
use whale_land::{Connection, Error, NewObjectId, Packet};
use whale_land::protocol::EventHandler;
use tracing::{debug, instrument};


struct RegistryHandler;
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
