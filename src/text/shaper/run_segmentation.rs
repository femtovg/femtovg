
use std::str::Chars;
use std::iter::Peekable;

use harfbuzz_rs as hb;

use unicode_script::{
    Script,
    UnicodeScript
};

use unicode_bidi::{
    bidi_class,
    BidiClass
};

use super::Direction;

impl From<BidiClass> for Direction {
    fn from(class: BidiClass) -> Self {
        match class {
            BidiClass::L => Direction::Ltr,
            BidiClass::R => Direction::Rtl,
            BidiClass::AL => Direction::Rtl,
            _ => Direction::Ltr
        }
    }
}

#[derive(Clone, Debug)]
pub struct Segment {
    //face: &'a Face,
    pub direction: Direction,
    pub script: Script,
    pub text: String,
    // language
}

impl Segment {
    pub fn hb_buffer(&self) -> hb::UnicodeBuffer {
        let mut buffer = hb::UnicodeBuffer::new()
            .add_str(&self.text)
            .set_direction(match self.direction {
                Direction::Ltr => hb::Direction::Ltr,
                Direction::Rtl => hb::Direction::Rtl,
            });

        let script_name = self.script.short_name();

        if script_name.len() == 4 {
            let script: Vec<char> = script_name.chars().collect();
            buffer = buffer.set_script(hb::Tag::new(script[0], script[1], script[2], script[3]));
        }

        buffer
    }
}

pub struct SegmentsIterator<I: Iterator<Item = char>> {
    iter: Peekable<I>
}

impl<I: Iterator<Item = char>> SegmentsIterator<I> {
    pub fn new(iter: I) -> Self {
        SegmentsIterator {
            iter: iter.peekable()
        }
    }
}

impl<I: Iterator<Item = char>> Iterator for SegmentsIterator<I> {
    type Item = Segment;

    fn next(&mut self) -> Option<Self::Item> {

        if let Some(first) = self.iter.next() {
            let mut script = first.script();
            let direction = Direction::from(bidi_class(first));
            let mut text = String::new();
            text.push(first);

            while let Some(next) = self.iter.peek() {
                //let next_dir = Direction::from(bidi_class(*next));
                let next_script = next.script();

                let next_script = match next_script {
                    Script::Common => script,
                    Script::Inherited => script,
                    _ => next_script
                };

                script = match script {
                    Script::Common => next_script,
                    Script::Inherited => next_script,
                    _ => script
                };

                if next_script == script/* && next_dir == direction*/ {
                    text.push(self.iter.next().unwrap());
                } else {
                    break;
                }
            }

            return Some(Segment {
                direction: direction,
                script: script,
                text: text
            });
        }

        None
    }
}

pub trait Segmentable<I: Iterator<Item = char>> {
    fn segments(self) -> SegmentsIterator<I>;
}

impl<'a> Segmentable<Chars<'a>> for &'a str {
    fn segments(self) -> SegmentsIterator<Chars<'a>> {
        SegmentsIterator::new(self.chars())
    }
}

impl<I: Iterator<Item=char>> Segmentable<I> for I {
    fn segments(self) -> SegmentsIterator<I> {
        SegmentsIterator::new(self)
    }
}
