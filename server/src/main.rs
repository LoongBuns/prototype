use protocol::Config;
use server::run;

#[tokio::main]
async fn main() {
    let Config { host, port, .. } = Config::new();

    run(&host, port).await;
}
