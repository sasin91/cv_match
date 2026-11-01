//! A page's text as it reads, from sappkit's pdfreader (text.rs), with its search, selection
//! and screen reader parts taken out: the page's runs gathered into lines, the lines put in
//! reading order, and two columns one after the other when the page has a gutter between them,
//! as a CV with a side column has.
//!
//! Positions are the page's: points from its top left corner, as `TextRun`s have them.

use super::page::{Page, TextRun};

/// How far apart baselines may be and still make one line, as a share of
/// the larger size: superscripts and footnote marks stay on their line.
const SAME_LINE: f32 = 0.45;
/// A gap wider than this share of the size reads as a space.
const SPACE_GAP: f32 = 0.15;
/// The most x positions the gutter search tries. It tries every point across the middle of the
/// page, but a broken file's text can sit a trillion points out; past this many it tries evenly
/// spread ones instead, so the work stays bounded.
const MAX_GUTTER_SAMPLES: f32 = 2000.0;

/// The page's text in reading order, with spaces where runs on a line stand apart and lines
/// joined with `\n`.
pub fn page_text(page: &Page) -> String {
    let texts = &page.texts;
    let mut out = String::new();
    for (index, line) in lines(page).iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let mut previous: Option<&TextRun> = None;
        for &run in line {
            let t = &texts[run];
            if let Some(p) = previous {
                let end = p.chars.last().map_or(p.x + p.width, |c| c.1);
                let gap = t.x - end;
                if gap > SPACE_GAP * p.size.max(t.size)
                    && !p.text.ends_with(char::is_whitespace)
                    && !t.text.starts_with(char::is_whitespace)
                {
                    out.push(' ');
                }
            }
            out.push_str(&t.text);
            previous = Some(t);
        }
    }
    out
}

fn same_line(a: &TextRun, b: &TextRun) -> bool {
    (a.baseline - b.baseline).abs() < SAME_LINE * a.size.max(b.size)
}

/// The page's runs as lines in reading order, each line's runs left to
/// right, as indices into `page.texts`.
fn lines(page: &Page) -> Vec<Vec<usize>> {
    let texts = &page.texts;
    let n = texts.len();
    let Some(g) = gutter(texts) else {
        return cluster(texts, (0..n).collect());
    };
    // Runs across the gutter span both columns, and so does what shares
    // their line: titles, author lists, wide captions.
    let crossing: Vec<usize> = (0..n).filter(|&i| crosses(&texts[i], g)).collect();
    let spans: Vec<bool> = (0..n)
        .map(|i| crossing.iter().any(|&j| same_line(&texts[i], &texts[j])))
        .collect();
    let spanning = cluster(texts, (0..n).filter(|&i| spans[i]).collect());
    let reference = |line: &[usize]| {
        line.iter()
            .map(|&i| &texts[i])
            .max_by(|a, b| a.size.total_cmp(&b.size))
            .map_or(0.0, |t| t.baseline)
    };
    let span_baselines: Vec<f32> = spanning.iter().map(|l| reference(l)).collect();
    // Bands between spanning lines; in each, the left column, then the
    // right, then the spanning line below.
    let bands = spanning.len() + 1;
    let mut columns: Vec<Vec<usize>> = vec![vec![]; bands * 2];
    for i in (0..n).filter(|&i| !spans[i]) {
        let t = &texts[i];
        let band = span_baselines.iter().filter(|&&b| b < t.baseline).count();
        let column = usize::from(t.x >= g);
        columns[band * 2 + column].push(i);
    }
    let mut out = vec![];
    for (band, pair) in columns.chunks(2).enumerate() {
        for column in pair {
            out.extend(cluster(texts, column.clone()));
        }
        if let Some(line) = spanning.get(band) {
            out.push(line.clone());
        }
    }
    out
}

fn crosses(t: &TextRun, g: f32) -> bool {
    t.x < g - 1.0 && t.x + t.width > g + 1.0
}

/// The x of the gutter between two columns, if the page has one: a band
/// down the middle few runs cross, with plenty of text either side.
fn gutter(texts: &[TextRun]) -> Option<f32> {
    let n = texts.len();
    if n < 8 {
        return None;
    }
    let left = texts.iter().map(|t| t.x).fold(f32::MAX, f32::min);
    let right = texts.iter().map(|t| t.x + t.width).fold(f32::MIN, f32::max);
    let width = right - left;
    if !width.is_finite() || width < 100.0 {
        return None;
    }
    let (from, to) = (left + 0.3 * width, left + 0.7 * width);
    // Points between the x positions tried: one, unless that would be too many.
    let step = ((to - from) / MAX_GUTTER_SAMPLES).max(1.0);
    let steps = ((to - from) / step).ceil() as usize;
    // At each x: how many runs cross it, if there is text down both
    // sides of it.
    let counts: Vec<Option<usize>> = (0..=steps)
        .map(|s| {
            let g = from + s as f32 * step;
            let lefts = texts.iter().filter(|t| t.x + t.width <= g).count();
            let rights = texts.iter().filter(|t| t.x >= g).count();
            (lefts * 7 >= n && rights * 7 >= n).then(|| texts.iter().filter(|t| crosses(t, g)).count())
        })
        .collect();
    // Author lists and the like across the top cross it here and there:
    // a few more than the fewest still make the gutter.
    let fewest = counts.iter().flatten().min().copied()?;
    let few = fewest + (fewest / 4).max(2);
    // The widest stretch crossed by few runs.
    let (mut best, mut start) = ((0, 0), None);
    for (s, c) in counts.iter().enumerate().chain([(counts.len(), &None)]) {
        match (c.is_some_and(|c| c <= few), start) {
            (true, None) => start = Some(s),
            (false, Some(b)) => {
                if s - b > best.1 - best.0 {
                    best = (b, s);
                }
                start = None;
            }
            _ => {}
        }
    }
    let band = (best.1 - best.0) as f32 * step;
    let g = from + (best.0 + best.1 - 1) as f32 / 2.0 * step;
    // At most half the runs across: most lines of a single column would
    // cross.
    (band >= 4.0 && fewest * 2 <= n).then_some(g)
}

/// Runs gathered into lines, top to bottom, each left to right.
fn cluster(texts: &[TextRun], mut runs: Vec<usize>) -> Vec<Vec<usize>> {
    runs.sort_by(|&a, &b| texts[a].baseline.total_cmp(&texts[b].baseline));
    // Each line with the run it is measured by: its largest.
    let mut lines: Vec<(Vec<usize>, usize)> = vec![];
    for i in runs {
        match lines.last_mut() {
            Some((line, main)) if same_line(&texts[*main], &texts[i]) => {
                line.push(i);
                if texts[i].size > texts[*main].size {
                    *main = i;
                }
            }
            _ => lines.push((vec![i], i)),
        }
    }
    lines
        .into_iter()
        .map(|(mut line, _)| {
            line.sort_by(|&a, &b| texts[a].x.total_cmp(&texts[b].x));
            line
        })
        .collect()
}
