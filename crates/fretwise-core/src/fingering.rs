//! Which finger plays each spot of a scale pattern.
//!
//! This is the standard "one finger per fret" position playing: the index
//! finger sits on the first fret of the range you're practising (fret 1 for
//! open position) and each finger covers the next fret up, so frets p, p+1, p+2, p+3 are fingers 1, 2, 3, 4. A pattern
//! five frets wide is still one position. On a string that needs the fifth
//! fret, the pinky stretches up to it. If that string also uses the fourth
//! fret, the hand shifts up one fret on that string instead.
//!
//! Fingering depends on where on the neck you play, not only on the scale:
//! the same note is fingered differently in each position. That's why it's
//! worked out from the frets shown, not stored with the scale. Wider ranges
//! hold several overlapping positions with no single right answer, so they
//! get no fingering.

use crate::theory::Position;

/// The widest pattern one hand position covers, counting the stretch.
pub const MAX_SPAN: u8 = 4;

/// One finger per position, in the same order: 0 for an open string, else
/// 1 (index) to 4 (pinky). `first_fret` is where the practice range starts;
/// the index finger goes there if the notes allow. `None` if the fretted
/// notes span more than one hand position.
pub fn fingering(positions: &[Position], first_fret: u8) -> Option<Vec<u8>> {
    let fretted = || positions.iter().map(|p| p.fret).filter(|&f| f > 0);
    let (Some(low_note), Some(hi)) = (fretted().min(), fretted().max()) else {
        return Some(vec![0; positions.len()]);
    };
    if hi - low_note > MAX_SPAN {
        return None;
    }
    // Where the index finger sits: the range's first fret, unless that
    // leaves the top note out of reach.
    let anchor = first_fret.max(1);
    let lo = if anchor <= low_note && hi - anchor <= MAX_SPAN { anchor } else { low_note };
    let on_string = |string: usize, fret: u8| positions.iter().any(|p| p.string == string && p.fret == fret);
    let fingers = positions
        .iter()
        .map(|p| {
            if p.fret == 0 {
                return 0;
            }
            let step = p.fret - lo; // 0..=4
            if hi - lo < MAX_SPAN || !on_string(p.string, lo + 4) {
                return step + 1;
            }
            // This string reaches the fifth fret of the position.
            if !on_string(p.string, lo + 3) {
                (step + 1).min(4) // pinky stretch
            } else if !on_string(p.string, lo) {
                step // the hand moves up a fret
            } else {
                // Both ends and the fourth fret: stretch both ways.
                match step {
                    0 => 1,
                    4 => 4,
                    s => s.min(3),
                }
            }
        })
        .collect();
    Some(fingers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingers(spots: &[(usize, u8)]) -> Option<Vec<u8>> {
        fingers_from(spots, 0)
    }

    fn fingers_from(spots: &[(usize, u8)], first_fret: u8) -> Option<Vec<u8>> {
        let positions: Vec<Position> = spots.iter().map(|&(string, fret)| Position { string, fret }).collect();
        fingering(&positions, first_fret)
    }

    #[test]
    fn minor_pentatonic_box_one() {
        // A minor pentatonic at the 5th fret, low E to high E.
        let spots = [(0, 5), (0, 8), (1, 5), (1, 7), (2, 5), (2, 7), (3, 5), (3, 7), (4, 5), (4, 8), (5, 5), (5, 8)];
        assert_eq!(fingers(&spots).unwrap(), [1, 4, 1, 3, 1, 3, 1, 3, 1, 4, 1, 4]);
    }

    #[test]
    fn open_position_and_open_strings() {
        // C major, first position, on the A and D strings.
        let spots = [(1, 3), (2, 0), (2, 2), (2, 3)];
        assert_eq!(fingers(&spots).unwrap(), [3, 0, 2, 3]);
        assert_eq!(fingers(&[(0, 0), (5, 0)]).unwrap(), [0, 0]);
    }

    #[test]
    fn index_finger_starts_the_range() {
        // Range 5..8 but only frets 5 and 7 in use: still 1 and 3.
        assert_eq!(fingers_from(&[(1, 5), (1, 7)], 5).unwrap(), [1, 3]);
        // Range 4..7 with notes on 5 and 7: index on 4.
        assert_eq!(fingers_from(&[(1, 5), (1, 7)], 4).unwrap(), [2, 4]);
        // Range starts too low to reach the top note: anchor on the lowest note.
        assert_eq!(fingers_from(&[(1, 5), (1, 7)], 1).unwrap(), [1, 3]);
    }

    #[test]
    fn five_fret_patterns_stretch() {
        // Position 2..6. String 0 stretches the pinky to 6; string 1 has
        // the 5th and 6th frets, so the hand moves up; string 2 has both ends.
        let spots = [(0, 2), (0, 4), (0, 6), (1, 3), (1, 5), (1, 6), (2, 2), (2, 5), (2, 6), (3, 4)];
        assert_eq!(fingers(&spots).unwrap(), [1, 3, 4, 1, 3, 4, 1, 3, 4, 3]);
    }

    #[test]
    fn wider_ranges_have_no_single_fingering() {
        assert_eq!(fingers(&[(0, 3), (0, 8)]), None);
        assert_eq!(fingers(&[(0, 0), (0, 1), (0, 4)]).unwrap(), [0, 1, 4]);
    }
}
