use protocol::Config;
use server::run;

#[tokio::main]
async fn main() {
    let Config { host, port, .. } = Config::new();

    env_logger::init();

    run(&host, port).await;
}
