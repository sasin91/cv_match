//! What changed between two versions of a text, word by word, for showing a revision with its
//! changes marked: the longest run of words both share (LCS) stays, the rest is added or removed.

/// Most cells the word table may have (words before × words after, once the common start and
/// end are cut off): beyond it, the middle is shown as removed and added whole, so a résumé
/// rewritten from top to bottom can't take long or much memory.
const MAX_CELLS: usize = 4_000_000;

/// A run of text: as it was, added, or removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Same(String),
    Added(String),
    Removed(String),
}

/// The text's words, each with the whitespace after it, so the pieces join back into the text.
fn words(text: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (at, c) in text.char_indices() {
        if c.is_whitespace() {
            in_space = true;
        } else if in_space {
            words.push(&text[start..at]);
            start = at;
            in_space = false;
        }
    }
    if start < text.len() {
        words.push(&text[start..]);
    }
    words
}

/// A word as compared: without the whitespace after it, so a word moved to the end of a line
/// is still the same word.
fn bare(word: &str) -> &str {
    word.trim_end()
}

/// How many words at the start, and then at the end, before and after share.
fn common_ends(before: &[&str], after: &[&str]) -> (usize, usize) {
    let start = before.iter().zip(after).take_while(|(b, a)| bare(b) == bare(a)).count();
    let end = before[start..]
        .iter()
        .rev()
        .zip(after[start..].iter().rev())
        .take_while(|(b, a)| bare(b) == bare(a))
        .count();
    (start, end)
}

/// The middle's changes from the longest common run of words.
fn middle(before: &[&str], after: &[&str], out: &mut Vec<Change>) {
    let (n, m) = (before.len(), after.len());
    if n * m > MAX_CELLS {
        push(out, Change::Removed(before.concat()));
        push(out, Change::Added(after.concat()));
        return;
    }
    // longest[i][j]: the longest common run of before[i..] and after[j..].
    let width = m + 1;
    let mut longest = vec![0u32; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            longest[i * width + j] = if bare(before[i]) == bare(after[j]) {
                longest[(i + 1) * width + j + 1] + 1
            } else {
                longest[(i + 1) * width + j].max(longest[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if bare(before[i]) == bare(after[j]) {
            push(out, Change::Same(after[j].to_string()));
            i += 1;
            j += 1;
        } else if longest[(i + 1) * width + j] >= longest[i * width + j + 1] {
            push(out, Change::Removed(before[i].to_string()));
            i += 1;
        } else {
            push(out, Change::Added(after[j].to_string()));
            j += 1;
        }
    }
    for word in &before[i..] {
        push(out, Change::Removed((*word).to_string()));
    }
    for word in &after[j..] {
        push(out, Change::Added((*word).to_string()));
    }
}

/// Adds a piece, joined to the last one when it is the same kind.
fn push(out: &mut Vec<Change>, change: Change) {
    match (out.last_mut(), &change) {
        (Some(Change::Same(last)), Change::Same(more))
        | (Some(Change::Added(last)), Change::Added(more))
        | (Some(Change::Removed(last)), Change::Removed(more)) => last.push_str(more),
        _ => {
            let empty = matches!(&change, Change::Same(t) | Change::Added(t) | Change::Removed(t) if t.is_empty());
            if !empty {
                out.push(change);
            }
        }
    }
}

/// after as changed from before, in order: the words both have, and those added and removed.
pub fn changes(before: &str, after: &str) -> Vec<Change> {
    let (before, after) = (words(before), words(after));
    let (start, end) = common_ends(&before, &after);
    let mut out = Vec::new();
    push(&mut out, Change::Same(after[..start].concat()));
    middle(
        &before[start..before.len() - end],
        &after[start..after.len() - end],
        &mut out,
    );
    push(&mut out, Change::Same(after[after.len() - end..].concat()));
    out
}

/// Whether anything changed at all.
pub fn any(changes: &[Change]) -> bool {
    changes.iter().any(|change| !matches!(change, Change::Same(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(t: &str) -> Change {
        Change::Same(t.to_string())
    }
    fn added(t: &str) -> Change {
        Change::Added(t.to_string())
    }
    fn removed(t: &str) -> Change {
        Change::Removed(t.to_string())
    }

    #[test]
    fn a_changed_word_is_removed_and_added_in_place() {
        assert_eq!(
            changes(
                "I have worked in software for 16 years.",
                "I have worked in software for 9 years."
            ),
            vec![
                same("I have worked in software for "),
                removed("16 "),
                added("9 "),
                same("years.")
            ]
        );
    }

    #[test]
    fn the_same_text_has_no_changes() {
        let text = "Kære Acme\n\nJeg søger stillingen.";
        assert_eq!(changes(text, text), vec![same(text)]);
        assert!(!any(&changes(text, text)));
    }

    #[test]
    fn the_pieces_join_back_into_the_new_text() {
        let before = "One two three\nfour five six";
        let after = "One three\nfour and five six seven";
        let joined: String = changes(before, after)
            .iter()
            .filter_map(|change| match change {
                Change::Same(t) | Change::Added(t) => Some(t.as_str()),
                Change::Removed(_) => None,
            })
            .collect();
        assert_eq!(joined, after);
    }

    #[test]
    fn a_word_moved_to_another_line_is_the_same_word() {
        assert_eq!(
            changes("Dear Acme team", "Dear Acme\nteam"),
            vec![same("Dear Acme\nteam")]
        );
    }

    #[test]
    fn empty_texts_are_all_added_or_removed() {
        assert_eq!(changes("", "New"), vec![added("New")]);
        assert_eq!(changes("Old", ""), vec![removed("Old")]);
    }
}
