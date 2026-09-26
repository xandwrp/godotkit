//! "Did you mean" ranking shared by xref, the API index, and engine diagnostics.
//!
//! # Tests (inline)
//! - `transposition_is_one_edit`
//! - `closest_first_then_alphabetical_and_the_name_itself_is_skipped`

/// Candidates within a small edit distance of `name` (a third of its length,
/// between 1 and 3 edits), closest first, then alphabetically; case-insensitive.
/// At most `limit`, without duplicates or `name` itself.
pub fn similar<'n>(
    name: &str,
    candidates: impl IntoIterator<Item = &'n str>,
    limit: usize,
) -> Vec<String> {
    let query = name.to_lowercase();
    let threshold = (query.chars().count() / 3).clamp(1, 3);
    let mut scored: Vec<(usize, &str)> = candidates
        .into_iter()
        .filter(|candidate| *candidate != name)
        .map(|candidate| (edit_distance(&query, &candidate.to_lowercase()), candidate))
        .filter(|(distance, _)| *distance <= threshold)
        .collect();
    scored.sort_unstable();
    scored.dedup();
    scored
        .into_iter()
        .take(limit)
        .map(|(_, candidate)| candidate.to_owned())
        .collect()
}

/// Optimal string alignment distance: Levenshtein plus adjacent transpositions,
/// so the commonest typo (`Lable`) is one edit away.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in rows.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in rows[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (rows[i - 1][j] + 1)
                .min(rows[i][j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(rows[i - 2][j - 2] + 1);
            }
            rows[i][j] = best;
        }
    }
    rows[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transposition_is_one_edit() {
        assert_eq!(edit_distance("lable", "label"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn closest_first_then_alphabetical_and_the_name_itself_is_skipped() {
        let found = similar(
            "queue_fre",
            [
                "queue_free",
                "queue_fre",
                "queue_redraw",
                "Queue_Free",
                "queue_frees",
            ],
            5,
        );
        assert_eq!(found, ["Queue_Free", "queue_free", "queue_frees"]);
        assert_eq!(similar("x", ["y", "zz"], 5), ["y"]);
    }
}
