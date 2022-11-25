use wad::Wad;

const WAD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_wads/doom.wad");

fn main() {
    let wad = Wad::new(WAD_PATH).unwrap();
    for item in wad.directory.iter() {
        dbg!(item);
    }
}
