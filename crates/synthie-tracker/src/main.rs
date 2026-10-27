mod app;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod tracker;
#[cfg(target_arch = "wasm32")]
mod web;

/// Number of decimated samples held in the scope display buffer.
const SCOPE_CAPACITY: usize = 512;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::main()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::main();
}
