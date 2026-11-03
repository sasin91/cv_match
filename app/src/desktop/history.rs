//! The places the window was on, newest last, for the mouse's back button: Back goes to the
//! place before, as a browser's does.

use std::collections::VecDeque;

/// How many places Back can go back through; older ones are forgotten.
const PLACES_KEPT: usize = 50;

/// The places left, newest last.
#[derive(Debug)]
pub struct History<Place> {
    places: VecDeque<Place>,
}

impl<Place: PartialEq> History<Place> {
    pub fn new() -> Self {
        Self {
            places: VecDeque::new(),
        }
    }

    /// Notes the place the window just left. Leaving the same place twice in a row (a screen
    /// left, then left again after a Back to it) keeps it once.
    pub fn left(&mut self, place: Place) {
        if self.places.back() == Some(&place) {
            return;
        }
        if self.places.len() == PLACES_KEPT {
            self.places.pop_front();
        }
        self.places.push_back(place);
    }

    /// The place before the one shown, taken out; None when there is none.
    pub fn back(&mut self) -> Option<Place> {
        self.places.pop_back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_goes_through_the_places_left_newest_first() {
        let mut history = History::new();
        history.left(1);
        history.left(2);
        assert_eq!(history.back(), Some(2));
        assert_eq!(history.back(), Some(1));
        assert_eq!(history.back(), None);
    }

    #[test]
    fn the_same_place_left_twice_in_a_row_is_kept_once() {
        let mut history = History::new();
        history.left(1);
        history.left(1);
        assert_eq!(history.back(), Some(1));
        assert_eq!(history.back(), None);
    }

    #[test]
    fn only_the_newest_places_are_kept() {
        let mut history = History::new();
        for place in 0..PLACES_KEPT + 1 {
            history.left(place);
        }
        let mut kept = 0;
        while let Some(place) = history.back() {
            assert_ne!(place, 0, "the oldest place is forgotten");
            kept += 1;
        }
        assert_eq!(kept, PLACES_KEPT);
    }
}
