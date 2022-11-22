use wad::Wad;

fn main() {
    let wad = Wad::new("/home/kyle/doom/iwad/doom1.wad").unwrap();
    dbg!(wad.directory);
}
