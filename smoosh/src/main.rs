use wad::Wad;

const WAD_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../test_wads/doom.wad");

fn main() {
    let wad = Wad::new(WAD_PATH).unwrap();
    wad.write("doom_copy.wad").unwrap();
}
