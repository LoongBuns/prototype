use protocol::Config;
use server::run;

#[tokio::main]
async fn main() {
    let Config { host, inspector_port, dispatcher_port, .. } = Config::new();

    env_logger::init();

    run(&host, &[inspector_port, dispatcher_port]).await;
}
