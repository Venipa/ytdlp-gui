//! `cargo run -p engine --example fixture_server [-- <port>]`: serves tests/fixtures with Range support.
//! Try: http://127.0.0.1:<port>/sample.mp4, /hls/index.m3u8, /big.mp4?size=41943040&slow=20
#[path = "../tests/common/server.rs"]
mod server;

fn main() {
    let port = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(8765);
    let s = server::start(server::fixtures_dir(), port);
    println!("fixture server on http://{}", s.addr);
    loop {
        std::thread::park();
    }
}
