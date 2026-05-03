use whale_land::Connection;


async fn run() {
    // connect to Wayland
    let wl = Connection::new_from_env()
        .await.expect("failed to connect to Wayland");

    loop {
        let packet = wl.recv_packet()
            .await.expect("failed to receive Wayland packet");
        println!("{:?}", packet);
        wl.dispatch(packet)
            .await.expect("failed to dispatch Wayland packet");
    };
}


#[tokio::main]
async fn main() {
    run().await
}
