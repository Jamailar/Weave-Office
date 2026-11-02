//! "Did you mean" for command ids.

/// Up to `n` of `candidates` closest to `wanted`: ids containing it first, then by edit distance.
pub fn closest<'a>(
    wanted: &str,
    candidates: impl Iterator<Item = &'a str>,
    n: usize,
) -> Vec<String> {
    let w = wanted.to_ascii_lowercase();
    let mut scored: Vec<(usize, &str)> = candidates
        .filter_map(|c| {
            let l = c.to_ascii_lowercase();
            let tail = l.rsplit('.').next().unwrap_or(&l);
            let wtail = w.rsplit('.').next().unwrap_or(&w);
            let score = if l.contains(&w) || (!wtail.is_empty() && tail.contains(wtail)) {
                0
            } else {
                distance(&w, &l).min(distance(wtail, tail) + 1)
            };
            (score <= (w.len() / 3).max(2)).then_some((score, c))
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.len().cmp(&b.1.len()))
            .then(a.1.cmp(b.1))
    });
    scored
        .into_iter()
        .take(n)
        .map(|(_, c)| c.to_string())
        .collect()
}

/// Levenshtein distance.
pub fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != *cb);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typo_finds_the_command() {
        let ids = [
            "format.bold",
            "format.italic",
            "insert.table",
            "text.newParagraph",
        ];
        assert_eq!(closest("format.bols", ids.into_iter(), 3)[0], "format.bold");
        assert_eq!(closest("table", ids.into_iter(), 3), vec!["insert.table"]);
        assert!(closest("zzzzzzzzzz", ids.into_iter(), 3).is_empty());
    }
}
