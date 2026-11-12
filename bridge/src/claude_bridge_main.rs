// Entry point of the headless Claude bridge. Usage: rustdesk-claude [port]
fn main() {
    let port = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    librustdesk::claude_bridge::run(port);
}
