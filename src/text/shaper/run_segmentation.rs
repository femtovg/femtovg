
use std::str::Chars;
use std::iter::Peekable;

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

// TODO: Make this borrow a &str instead of allocating a String every time
pub struct UnicodeScriptIterator<I: Iterator<Item = char>> {
    iter: Peekable<I>
}

impl<I: Iterator<Item = char>> Iterator for UnicodeScriptIterator<I> {
    type Item = (Script, Direction, String);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(first) = self.iter.next() {
            let direction = Direction::from(bidi_class(first));
            let mut script = first.script();
            let mut text = String::new();
            text.push(first);

            while let Some(next) = self.iter.peek() {
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

                if next_script == script {
                    text.push(self.iter.next().unwrap());
                } else {
                    break;
                }
            }

            return Some((script, direction, text));
        }

        None
    }
}

pub trait UnicodeScripts<I: Iterator<Item = char>> {
    fn unicode_scripts(self) -> UnicodeScriptIterator<I>;
}

impl<'a> UnicodeScripts<Chars<'a>> for &'a str {
    fn unicode_scripts(self) -> UnicodeScriptIterator<Chars<'a>> {
        UnicodeScriptIterator {
            iter: self.chars().peekable()
        }
    }
}

impl<I: Iterator<Item=char>> UnicodeScripts<I> for I {
    fn unicode_scripts(self) -> UnicodeScriptIterator<I> {
        UnicodeScriptIterator {
            iter: self.peekable()
        }
    }
}
