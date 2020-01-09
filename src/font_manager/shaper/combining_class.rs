
// Links:
// https://www.compart.com/en/unicode/combining

#[derive(Debug)]
pub enum CombiningClass {
    Above = 230,
    Bellow = 220,
    //Virama, Nukuta etc
}

impl CombiningClass {

    pub fn new(key: u8) -> Option<Self> {
        match key {
            230 => Some(Self::Above),
            220 => Some(Self::Bellow),
            _ => None
        }
    }

}
