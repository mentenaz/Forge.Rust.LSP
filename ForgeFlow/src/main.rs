//! ForgeFlow language server.

mod grammar;
mod semantic;
mod server;

fn main() {
    server::run();
}
