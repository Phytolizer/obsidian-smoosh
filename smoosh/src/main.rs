use wad::Wad;

fn main() {
    let wad = Wad::new("/home/kyle/doom/iwad/doom1.wad").unwrap();
    for item in wad.directory.iter() {
        dbg!(item);
    }
}
