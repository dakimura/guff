//! Port of `github.com/ccojocar/zxcvbn-go` v1.0.4, the entropy estimator gosec
//! calls from G101.
//!
//! Only `PasswordStrength(s, nil).Entropy` is needed, so the crack-time and
//! score halves of upstream's `MinEntropyMatch` are left out. Everything that
//! feeds the entropy is here, quirks included — several of them decide real
//! findings:
//!
//!   * `l33tMatch` mutates a *copy* of each match, so the extra l33t entropy it
//!     computes is discarded and the match keeps its plain dictionary entropy.
//!   * `endUpperRx` is written `^[^A-Z]+[A-Z]$'`, with a trailing quote after
//!     the anchor, so it never matches anything.
//!   * `CalculateAvgDegree` counts the *characters* of each neighbour entry
//!     (`"2@"` counts 2) and divides by the number of entries, empty ones
//!     included.
//!   * date matches carry an end-exclusive `J` where every other matcher's is
//!     inclusive, and `dateWithoutSepMatchHelper` puts the whole password in
//!     the token.
//!
//! Byte and rune indexing follow the Go original: the dictionary matcher walks
//! runes, everything else indexes bytes.

use std::collections::HashMap;
use std::sync::LazyLock;

const ENGLISH: &str = include_str!("data/english.txt");
const FEMALE_NAMES: &str = include_str!("data/female_names.txt");
const MALE_NAMES: &str = include_str!("data/male_names.txt");
const SURNAMES: &str = include_str!("data/surnames.txt");
const PASSWORDS: &str = include_str!("data/passwords.txt");

const QWERTY: &str = include_str!("data/qwerty.txt");
const DVORAK: &str = include_str!("data/dvorak.txt");
const KEYPAD: &str = include_str!("data/keypad.txt");
const MAC_KEYPAD: &str = include_str!("data/mac_keypad.txt");
const L33T: &str = include_str!("data/l33t.txt");

/// `key<TAB>neighbour<TAB>neighbour…`, with an empty field where the JSON had
/// `null` — the position of each neighbour is the direction index the spatial
/// matcher counts turns with, so the gaps have to survive.
struct Graph {
    name: &'static str,
    map: HashMap<String, Vec<String>>,
}

impl Graph {
    fn parse(src: &str, name: &'static str) -> Self {
        let mut map = HashMap::new();
        for line in src.lines() {
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split('\t');
            let key = parts.next().unwrap_or_default().to_string();
            map.insert(key, parts.map(|s| s.to_string()).collect());
        }
        Graph { name, map }
    }

    /// `CalculateAvgDegree`.
    fn average_degree(&self) -> f64 {
        let mut avg = 0.0;
        let mut count = 0.0;
        for value in self.map.values() {
            for neighbour in value {
                avg += neighbour.chars().count() as f64;
                count += 1.0;
            }
        }
        avg / count
    }
}

/// Lower-cased word to its 1-based rank in the frequency list.
fn ranked_dict(src: &str) -> HashMap<String, usize> {
    src.lines()
        .enumerate()
        .map(|(i, w)| (w.to_lowercase(), i + 1))
        .collect()
}

struct Data {
    dictionaries: Vec<(&'static str, HashMap<String, usize>)>,
    graphs: Vec<Graph>,
    l33t: Graph,
    keypad_starting_positions: f64,
    keypad_avg_degree: f64,
    qwerty_starting_positions: f64,
    qwerty_avg_degree: f64,
}

static DATA: LazyLock<Data> = LazyLock::new(|| {
    let qwerty = Graph::parse(QWERTY, "qwerty");
    let keypad = Graph::parse(KEYPAD, "keypad");
    let qwerty_starting_positions = qwerty.map.len() as f64;
    let qwerty_avg_degree = qwerty.average_degree();
    let keypad_starting_positions = keypad.map.len() as f64;
    let keypad_avg_degree = keypad.average_degree();
    Data {
        // `frequency.Lists` is a Go map, so the order these are built in is
        // random; every match is scored on its own and the minimum is taken,
        // so the entropy does not depend on it.
        dictionaries: vec![
            ("MaleNames", ranked_dict(MALE_NAMES)),
            ("FemaleNames", ranked_dict(FEMALE_NAMES)),
            ("Surname", ranked_dict(SURNAMES)),
            ("English", ranked_dict(ENGLISH)),
            ("Passwords", ranked_dict(PASSWORDS)),
        ],
        graphs: vec![
            qwerty,
            Graph::parse(DVORAK, "dvorak"),
            keypad,
            Graph::parse(MAC_KEYPAD, "mac_keypad"),
        ],
        l33t: Graph::parse(L33T, "keypad"),
        keypad_starting_positions,
        keypad_avg_degree,
        qwerty_starting_positions,
        qwerty_avg_degree,
    }
});

#[derive(Clone)]
struct Match {
    pattern: &'static str,
    i: usize,
    j: usize,
    /// Bytes, as in Go: the byte-indexed matchers can slice through a
    /// multi-byte character, which Go allows and `&str` does not.
    token: Vec<u8>,
    dictionary_name: String,
    entropy: f64,
}

impl Match {
    fn token_str(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.token)
    }
}

/// `zxcvbn.PasswordStrength(password, nil).Entropy`.
pub fn entropy(password: &str) -> f64 {
    let matches = omnimatch(password);
    minimum_entropy_match_sequence(password, &matches)
}

fn omnimatch(password: &str) -> Vec<Match> {
    let mut matches = Vec::new();
    for (name, dict) in &DATA.dictionaries {
        matches.extend(dictionary_match(password, name, dict));
    }
    matches.extend(spatial_match(password));
    matches.extend(repeat_match(password));
    matches.extend(sequence_match(password));
    matches.extend(l33t_match(password));
    matches.extend(date_sep_match(password));
    matches.extend(date_without_sep_match(password));
    matches.sort_by(|a, b| a.i.cmp(&b.i).then(a.j.cmp(&b.j)));
    matches
}

// ---------------------------------------------------------------- scoring

fn minimum_entropy_match_sequence(password: &str, matches: &[Match]) -> f64 {
    let n = password.len();
    if n == 0 {
        return 0.0;
    }
    let bruteforce_cardinality = calc_brute_force_cardinality(password);
    let mut up_to_k = vec![0.0_f64; n];

    for k in 0..n {
        up_to_k[k] = get(&up_to_k, k as isize - 1) + bruteforce_cardinality.log2();
        for m in matches {
            if m.j != k {
                continue;
            }
            let candidate = get(&up_to_k, m.i as isize - 1) + m.entropy;
            if candidate < up_to_k[k] {
                up_to_k[k] = candidate;
            }
        }
    }
    round_to_x_digits(up_to_k[n - 1], 3)
}

fn get(a: &[f64], i: isize) -> f64 {
    if i < 0 || i as usize >= a.len() {
        return 0.0;
    }
    a[i as usize]
}

fn round_to_x_digits(value: f64, digits: i32) -> f64 {
    let pow = 10f64.powi(digits);
    let digit = pow * value;
    let frac = digit.fract();
    let rounded = if frac >= 0.5 {
        digit.ceil()
    } else {
        digit.floor()
    };
    rounded / pow
}

// ---------------------------------------------------------------- entropy

fn n_choose_k(mut n: f64, k: f64) -> f64 {
    if k > n {
        return 0.0;
    } else if k == 0.0 {
        return 1.0;
    }
    let mut r = 1.0_f64;
    let mut d = 1.0_f64;
    while d <= k {
        r *= n;
        r /= d;
        n -= 1.0;
        d += 1.0;
    }
    r
}

fn calc_brute_force_cardinality(password: &str) -> f64 {
    let (mut lower, mut upper, mut digits, mut symbols) = (0.0, 0.0, 0.0, 0.0);
    for ch in password.chars() {
        if ch.is_lowercase() {
            lower = 26.0;
        } else if ch.is_numeric() {
            digits = 10.0;
        } else if ch.is_uppercase() {
            upper = 26.0;
        } else {
            symbols = 33.0;
        }
    }
    lower + upper + digits + symbols
}

fn dictionary_entropy(m: &Match, rank: f64) -> f64 {
    rank.log2() + extra_upper_case_entropy(m)
}

fn extra_upper_case_entropy(m: &Match) -> f64 {
    let word = m.token_str();
    if !word.chars().any(|c| c.is_uppercase()) {
        return 0.0;
    }
    // `^[A-Z][^A-Z]+$` and `^[A-Z]+$`. The third pattern upstream tries,
    // `^[^A-Z]+[A-Z]$'`, cannot match anything.
    let chars: Vec<char> = word.chars().collect();
    let is_upper_ascii = |c: char| c.is_ascii_uppercase();
    let start_upper = chars.len() >= 2
        && is_upper_ascii(chars[0])
        && chars[1..].iter().all(|&c| !is_upper_ascii(c));
    let all_upper = !chars.is_empty() && chars.iter().all(|&c| is_upper_ascii(c));
    if start_upper || all_upper {
        return 1.0;
    }

    let mut count_upper = 0.0_f64;
    let mut count_lower = 0.0_f64;
    for ch in word.chars() {
        if ch.is_uppercase() {
            count_upper += 1.0;
        } else if ch.is_lowercase() {
            count_lower += 1.0;
        }
    }
    let total = count_lower + count_upper;
    let mut possibilities = 0.0;
    let mut i = 0.0;
    while i <= count_upper.min(count_lower) {
        possibilities += n_choose_k(total, i);
        i += 1.0;
    }
    if possibilities < 1.0 {
        return 1.0;
    }
    possibilities.log2()
}

fn spatial_entropy(m: &Match, turns: i64, shift_count: i64) -> f64 {
    let (s, d) = if m.dictionary_name == "qwerty" || m.dictionary_name == "dvorak" {
        (DATA.qwerty_starting_positions, DATA.qwerty_avg_degree)
    } else {
        (DATA.keypad_starting_positions, DATA.keypad_avg_degree)
    };

    let mut possibilities = 0.0_f64;
    let length = m.token.len() as f64;
    let mut i = 2.0_f64;
    while i <= length + 1.0 {
        let possible_turns = (turns as f64).min(i - 1.0);
        let mut j = 1.0_f64;
        while j <= possible_turns + 1.0 {
            possibilities += n_choose_k(i - 1.0, j - 1.0) * s * d.powf(j);
            j += 1.0;
        }
        i += 1.0;
    }

    let mut entropy = possibilities.log2();
    let shift = shift_count as f64;
    if shift > 0.0 {
        let mut possibilities = 0.0_f64;
        let unshifted = length - shift;
        let mut i = 0.0_f64;
        while i < shift.min(unshifted) + 1.0 {
            possibilities += n_choose_k(shift + unshifted, i);
            i += 1.0;
        }
        entropy += possibilities.log2();
    }
    entropy
}

fn repeat_entropy(m: &Match) -> f64 {
    (calc_brute_force_cardinality(&m.token_str()) * m.token.len() as f64).log2()
}

fn sequence_entropy(m: &Match, dictionary_length: usize, ascending: bool) -> f64 {
    let first = m.token[0];
    let mut base = if first == b'a' || first == b'1' {
        0.0
    } else {
        let mut b = (dictionary_length as f64).log2();
        if (first as char).is_ascii_uppercase() {
            b += 1.0;
        }
        b
    };
    if !ascending {
        base += 1.0;
    }
    base + (m.token.len() as f64).log2()
}

const NUM_YEARS: f64 = 119.0;
const NUM_MONTHS: f64 = 12.0;
const NUM_DAYS: f64 = 31.0;

fn date_entropy(year: i64, separator: &str) -> f64 {
    let mut entropy = if year < 100 {
        (NUM_DAYS * NUM_MONTHS * 100.0).log2()
    } else {
        (NUM_DAYS * NUM_MONTHS * NUM_YEARS).log2()
    };
    if !separator.is_empty() {
        entropy += 2.0;
    }
    entropy
}

// ---------------------------------------------------------------- matchers

fn dictionary_match(password: &str, name: &str, ranked: &HashMap<String, usize>) -> Vec<Match> {
    let lower: Vec<char> = password.to_lowercase().chars().collect();
    let original: Vec<char> = password.chars().collect();
    // `strings.ToLower` can change the rune count (it does not here for the
    // inputs G101 sees); fall back to the original when it does, as indexing
    // both would otherwise disagree.
    if lower.len() != original.len() {
        return Vec::new();
    }
    let mut results = Vec::new();
    for i in 0..lower.len() {
        for j in i..lower.len() {
            let word: String = lower[i..=j].iter().collect();
            if let Some(&rank) = ranked.get(&word) {
                let mut m = Match {
                    pattern: "dictionary",
                    i,
                    j,
                    token: original[i..=j].iter().collect::<String>().into_bytes(),
                    dictionary_name: name.to_string(),
                    entropy: 0.0,
                };
                m.entropy = dictionary_entropy(&m, rank as f64);
                results.push(m);
            }
        }
    }
    results
}

fn spatial_match(password: &str) -> Vec<Match> {
    let mut matches = Vec::new();
    for graph in &DATA.graphs {
        matches.extend(spatial_match_helper(password, graph));
    }
    matches
}

fn spatial_match_helper(password: &str, graph: &Graph) -> Vec<Match> {
    let bytes = password.as_bytes();
    let mut matches = Vec::new();
    if bytes.is_empty() {
        return matches;
    }
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        let mut j = i + 1;
        let mut last_direction: i64 = -99;
        let mut turns: i64 = 0;
        let mut shifted_count: i64 = 0;

        loop {
            let prev_char = bytes[j - 1] as char;
            let mut found = false;
            let mut found_direction: i64 = 0;
            let mut cur_direction: i64 = -1;
            let empty = Vec::new();
            let adjacents = graph.map.get(&prev_char.to_string()).unwrap_or(&empty);

            if j < bytes.len() {
                let cur_char = bytes[j] as char;
                let needle = cur_char.to_string();
                for adj in adjacents {
                    cur_direction += 1;
                    if let Some(idx) = adj.find(&needle) {
                        found = true;
                        found_direction = cur_direction;
                        if idx == 1 {
                            shifted_count += 1;
                        }
                        if last_direction != found_direction {
                            turns += 1;
                            last_direction = found_direction;
                        }
                        break;
                    }
                }
            }

            if found {
                j += 1;
            } else {
                if j - i > 2 {
                    let mut m = Match {
                        pattern: "spatial",
                        i,
                        j: j - 1,
                        token: bytes[i..j].to_vec(),
                        dictionary_name: graph.name.to_string(),
                        entropy: 0.0,
                    };
                    m.entropy = spatial_entropy(&m, turns, shifted_count);
                    matches.push(m);
                }
                i = j;
                break;
            }
        }
    }
    matches
}

fn repeat_match(password: &str) -> Vec<Match> {
    let mut matches = Vec::new();
    let mut current;
    let mut prev = String::new();
    let mut current_streak = 1usize;
    let mut last_i = 0usize;

    for (i, ch) in password.char_indices() {
        last_i = i;
        current = ch.to_string();
        if i == 0 {
            prev = current;
            continue;
        }
        if current.to_lowercase() == prev.to_lowercase() {
            current_streak += 1;
        } else if current_streak > 2 {
            let i_pos = i - current_streak;
            let j_pos = i - 1;
            let mut m = Match {
                pattern: "repeat",
                i: i_pos,
                j: j_pos,
                token: password.as_bytes()[i_pos..=j_pos].to_vec(),
                dictionary_name: prev.clone(),
                entropy: 0.0,
            };
            m.entropy = repeat_entropy(&m);
            matches.push(m);
            current_streak = 1;
        } else {
            current_streak = 1;
        }
        prev = current;
    }

    if current_streak > 2 {
        let i_pos = last_i + 1 - current_streak;
        let j_pos = last_i;
        let mut m = Match {
            pattern: "repeat",
            i: i_pos,
            j: j_pos,
            token: password.as_bytes()[i_pos..=j_pos].to_vec(),
            dictionary_name: prev,
            entropy: 0.0,
        };
        m.entropy = repeat_entropy(&m);
        matches.push(m);
    }
    matches
}

const SEQUENCES: &[(&str, &str)] = &[
    ("lower", "abcdefghijklmnopqrstuvwxyz"),
    ("upper", "ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
    ("digits", "0123456789"),
];

fn sequence_match(password: &str) -> Vec<Match> {
    let bytes = password.as_bytes();
    let mut matches = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let mut j = i + 1;
        let mut seq = "";
        let mut seq_name = "";
        let mut seq_direction: i64 = 0;

        for (name, candidate) in SEQUENCES {
            let i_n = candidate.find(bytes[i] as char).map(|x| x as i64).unwrap_or(-1);
            let j_n = if j < bytes.len() {
                candidate.find(bytes[j] as char).map(|x| x as i64).unwrap_or(-1)
            } else {
                -1
            };
            if i_n > -1 && j_n > -1 {
                let direction = j_n - i_n;
                if direction == 1 || direction == -1 {
                    seq = candidate;
                    seq_name = name;
                    seq_direction = direction;
                    break;
                }
            }
        }

        if !seq.is_empty() {
            loop {
                let (prev_n, cur_n) = if j < bytes.len() {
                    (
                        seq.find(bytes[j - 1] as char).map(|x| x as i64).unwrap_or(-1),
                        seq.find(bytes[j] as char).map(|x| x as i64).unwrap_or(-1),
                    )
                } else {
                    (0, 0)
                };
                if j == bytes.len() || cur_n - prev_n != seq_direction {
                    if j - i > 2 {
                        let mut m = Match {
                            pattern: "sequence",
                            i,
                            j: j - 1,
                            token: bytes[i..j].to_vec(),
                            dictionary_name: seq_name.to_string(),
                            entropy: 0.0,
                        };
                        m.entropy = sequence_entropy(&m, seq.len(), seq_direction == 1);
                        matches.push(m);
                    }
                    break;
                }
                j += 1;
            }
        }
        i = j;
    }
    matches
}

/// The l33t matcher runs the dictionaries over every un-substituted spelling of
/// the password. Upstream then adds the extra l33t entropy to a *copy* of each
/// match and throws it away, so the matches keep their plain dictionary
/// entropy — reproduced here by not adding it.
fn l33t_match(password: &str) -> Vec<Match> {
    let mut matches = Vec::new();
    for permutation in get_permutations(password) {
        for (name, dict) in &DATA.dictionaries {
            matches.extend(dictionary_match(&permutation, name, dict));
        }
    }
    matches
}

fn get_permutations(password: &str) -> Vec<String> {
    let table = relevant_l33t_subtable(password);
    all_permutations_of_leet_substitutions(password, &table)
}

fn relevant_l33t_subtable(password: &str) -> Vec<(String, Vec<String>)> {
    let mut relevant: Vec<(String, Vec<String>)> = Vec::new();
    let mut keys: Vec<&String> = DATA.l33t.map.keys().collect();
    keys.sort();
    for key in keys {
        let values = &DATA.l33t.map[key];
        let hits: Vec<String> = values
            .iter()
            .filter(|v| password.contains(v.as_str()))
            .cloned()
            .collect();
        if !hits.is_empty() {
            relevant.push((key.clone(), hits));
        }
    }
    relevant
}

fn all_permutations_of_leet_substitutions(
    password: &str,
    table: &[(String, Vec<String>)],
) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for no_conflicts in list_of_tables_without_conflicts(table) {
        for substitutions in substitutions_maps_from_table(&no_conflicts) {
            let word = word_for_substitution_map(password, &substitutions);
            if !result.contains(&word) {
                result.push(word);
            }
        }
    }
    result
}

fn list_of_tables_without_conflicts(
    table: &[(String, Vec<String>)],
) -> Vec<Vec<(String, Vec<String>)>> {
    let mut result: Vec<Vec<(String, Vec<String>)>> = vec![table.to_vec()];
    for conflict in conflicts_list(table) {
        let mut next = Vec::new();
        for current in &result {
            next.extend(different_tables_for_leet_char(current, &conflict));
        }
        result = next;
    }
    result
}

fn conflicts_list(table: &[(String, Vec<String>)]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut found: Vec<String> = Vec::new();
    for (_, values) in table {
        for value in values {
            if found.contains(value) {
                if !result.contains(value) {
                    result.push(value.clone());
                }
            } else {
                found.push(value.clone());
            }
        }
    }
    result
}

fn different_tables_for_leet_char(
    table: &[(String, Vec<String>)],
    leet_char: &str,
) -> Vec<Vec<(String, Vec<String>)>> {
    let mut result = Vec::new();
    for key in keys_with_value(table, leet_char) {
        result.push(table_without_value_on_other_keys(table, &key, leet_char));
    }
    result
}

fn keys_with_value(table: &[(String, Vec<String>)], value_to_find: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for (key, values) in table {
        for value in values {
            if value == value_to_find && !result.contains(key) {
                result.push(key.clone());
            }
        }
    }
    result
}

fn table_without_value_on_other_keys(
    table: &[(String, Vec<String>)],
    key_to_fix: &str,
    value_to_fix: &str,
) -> Vec<(String, Vec<String>)> {
    let mut result: Vec<(String, Vec<String>)> = Vec::new();
    for (key, values) in table {
        let kept: Vec<String> = values
            .iter()
            .filter(|v| v.as_str() != value_to_fix || key == key_to_fix)
            .cloned()
            .collect();
        if !kept.is_empty() {
            result.push((key.clone(), kept));
        }
    }
    result
}

fn substitutions_maps_from_table(table: &[(String, Vec<String>)]) -> Vec<Vec<(String, String)>> {
    let mut result: Vec<Vec<(String, String)>> = vec![Vec::new()];
    let mut any = false;
    for (key, values) in table {
        let mut next = Vec::new();
        for current in &result {
            for value in values {
                let mut copy = current.clone();
                copy.push((key.clone(), value.clone()));
                next.push(copy);
            }
        }
        result = next;
        any = true;
    }
    if !any {
        return Vec::new();
    }
    result
}

fn word_for_substitution_map(word: &str, substitutions: &[(String, String)]) -> String {
    let mut result = word.to_string();
    for (key, value) in substitutions {
        result = result.replace(value.as_str(), key);
    }
    result
}

// ---------------------------------------------------------------- dates

fn check_date(day: i64, month: i64, year: i64) -> Option<(i64, i64, i64)> {
    let (mut day, mut month) = (day, month);
    if (12..=31).contains(&month) && day <= 12 {
        std::mem::swap(&mut day, &mut month);
    }
    if day > 31 || month > 12 {
        return None;
    }
    if !((1900..=2025).contains(&year) || (0..=99).contains(&year)) {
        return None;
    }
    Some((day, month, year))
}

/// `((\d{1,2})(sep)(\d{1,2})(sep)(19\d{2}|200\d|201\d|\d{2}))` and the
/// year-first spelling, hand-matched: the only regex features used are digit
/// runs and a six-character separator class.
fn scan_date_sep(bytes: &[u8], year_first: bool) -> Vec<(usize, usize, i64, i64, i64, String)> {
    fn digits(bytes: &[u8], at: usize, max: usize) -> usize {
        let mut n = 0;
        while n < max && at + n < bytes.len() && bytes[at + n].is_ascii_digit() {
            n += 1;
        }
        n
    }
    fn is_sep(b: u8) -> bool {
        matches!(b, b' ' | b'-' | b'/' | b'\\' | b'_' | b'.')
    }
    fn year_at(bytes: &[u8], at: usize) -> Option<usize> {
        // 19\d{2} | 200\d | 201\d | \d{2}
        let four = digits(bytes, at, 4);
        if four >= 4 {
            let s = &bytes[at..at + 4];
            if (s[0] == b'1' && s[1] == b'9')
                || (s[0] == b'2' && s[1] == b'0' && (s[2] == b'0' || s[2] == b'1'))
            {
                return Some(4);
            }
        }
        if four >= 2 { Some(2) } else { None }
    }

    let mut out = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let mut cursor = start;
        let mut parsed = None;
        if year_first {
            if let Some(ylen) = year_at(bytes, cursor) {
                let year: i64 = std::str::from_utf8(&bytes[cursor..cursor + ylen])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(-1);
                cursor += ylen;
                if cursor < bytes.len() && is_sep(bytes[cursor]) {
                    let sep = bytes[cursor] as char;
                    cursor += 1;
                    let d1 = digits(bytes, cursor, 2);
                    if d1 > 0 {
                        let month: i64 = std::str::from_utf8(&bytes[cursor..cursor + d1])
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(-1);
                        cursor += d1;
                        if cursor < bytes.len() && is_sep(bytes[cursor]) {
                            cursor += 1;
                            let d2 = digits(bytes, cursor, 2);
                            if d2 > 0 {
                                let day: i64 = std::str::from_utf8(&bytes[cursor..cursor + d2])
                                    .ok()
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(-1);
                                cursor += d2;
                                parsed = Some((day, month, year, sep.to_string()));
                            }
                        }
                    }
                }
            }
        } else {
            let d1 = digits(bytes, cursor, 2);
            if d1 > 0 {
                let month: i64 = std::str::from_utf8(&bytes[cursor..cursor + d1])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(-1);
                cursor += d1;
                if cursor < bytes.len() && is_sep(bytes[cursor]) {
                    cursor += 1;
                    let d2 = digits(bytes, cursor, 2);
                    if d2 > 0 {
                        let day: i64 = std::str::from_utf8(&bytes[cursor..cursor + d2])
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(-1);
                        cursor += d2;
                        if cursor < bytes.len() && is_sep(bytes[cursor]) {
                            let sep = bytes[cursor] as char;
                            cursor += 1;
                            if let Some(ylen) = year_at(bytes, cursor) {
                                let year: i64 =
                                    std::str::from_utf8(&bytes[cursor..cursor + ylen])
                                        .ok()
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or(-1);
                                cursor += ylen;
                                parsed = Some((day, month, year, sep.to_string()));
                            }
                        }
                    }
                }
            }
        }

        if let Some((day, month, year, sep)) = parsed {
            out.push((start, cursor, day, month, year, sep));
            start = cursor;
        } else {
            start += 1;
        }
    }
    out
}

fn date_sep_match(password: &str) -> Vec<Match> {
    let bytes = password.as_bytes();
    let mut matches = Vec::new();
    for year_first in [false, true] {
        for (i, j, day, month, year, sep) in scan_date_sep(bytes, year_first) {
            if let Some((_, _, year)) = check_date(day, month, year) {
                matches.push(Match {
                    pattern: "",
                    i,
                    j,
                    token: bytes[i..j].to_vec(),
                    dictionary_name: "date_match".to_string(),
                    entropy: date_entropy(year, &sep),
                });
            }
        }
    }
    matches
}

fn date_without_sep_match(password: &str) -> Vec<Match> {
    let bytes = password.as_bytes();
    let mut matches = Vec::new();
    // `\d{4,8}`, leftmost-longest as Go's regexp finds it.
    let mut at = 0usize;
    while at < bytes.len() {
        if !bytes[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < bytes.len() && bytes[end].is_ascii_digit() && end - at < 8 {
            end += 1;
        }
        if end - at < 4 {
            at = end.max(at + 1);
            continue;
        }
        let v = &password[at..end];
        let i = password.find(v).unwrap_or(at);
        let j = i + v.len();
        let length = v.len();
        let last_index = length - 1;
        let mut round_one: Vec<(String, String)> = Vec::new();
        if length <= 6 {
            round_one.push((v[2..].to_string(), v[0..2].to_string()));
            round_one.push((
                v[0..last_index - 2].to_string(),
                v[last_index - 2..].to_string(),
            ));
        }
        if length >= 6 {
            round_one.push((v[4..].to_string(), v[0..4].to_string()));
            round_one.push((
                v[0..last_index - 3].to_string(),
                v[last_index - 3..].to_string(),
            ));
        }

        let mut round_two: Vec<(String, String, String)> = Vec::new();
        for (day_month, year) in &round_one {
            match day_month.len() {
                // `c.DayMonth[0:0]` and `[1:1]` are both empty upstream.
                2 => round_two.push((String::new(), String::new(), year.clone())),
                3 => {
                    round_two.push((day_month[0..2].to_string(), String::new(), year.clone()));
                    round_two.push((String::new(), day_month[1..3].to_string(), year.clone()));
                }
                4 => round_two.push((
                    day_month[0..2].to_string(),
                    day_month[2..4].to_string(),
                    year.clone(),
                )),
                _ => {}
            }
        }

        for (day, month, year) in round_two {
            let (Ok(day), Ok(month), Ok(year)) = (
                day.parse::<i64>(),
                month.parse::<i64>(),
                year.parse::<i64>(),
            ) else {
                continue;
            };
            if let Some((_, _, checked_year)) = check_date(day, month, year) {
                let _ = checked_year;
                matches.push(Match {
                    pattern: "",
                    i,
                    j,
                    // Upstream stores the whole password as the token here.
                    token: password.as_bytes().to_vec(),
                    dictionary_name: "date_match".to_string(),
                    entropy: date_entropy(year, ""),
                });
            }
        }
        at = end;
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Differential harness against the Go original: set `ZXCVBN_IN` to a file
    /// of one string per line and `ZXCVBN_OUT` to where the entropies should
    /// be written. Not run in CI (no Go there); used while porting.
    #[test]
    fn differential_dump() {
        let (Ok(input), Ok(output)) = (
            std::env::var("ZXCVBN_IN"),
            std::env::var("ZXCVBN_OUT"),
        ) else {
            return;
        };
        let src = std::fs::read_to_string(input).expect("read input");
        let mut out = String::new();
        for line in src.lines() {
            if line.is_empty() {
                continue;
            }
            let s: String = line.chars().take_while(|_| true).collect();
            let truncated = if s.len() > 16 { &s[..s.floor_char_boundary(16)] } else { &s };
            out.push_str(&format!("{:.3}\n", entropy(truncated)));
        }
        std::fs::write(output, out).expect("write output");
    }

    /// Measured with `zxcvbn.PasswordStrength(s, []string{}).Entropy` at
    /// v1.0.4 — the strings gosec's G101 decides on in dapr and atlas.
    #[test]
    fn matches_upstream_entropy() {
        let cases: &[(&str, f64)] = &[
            ("secretStoreName", 26.148),
            ("mockSecretStore", 28.713),
            ("NAME1:SECRETKEY1", 34.884),
            ("local-secret-sto", 36.087),
            ("timestamp with t", 39.777),
            ("/var/run/dapr/cr", 77.833),
            ("DAPR_API_TOKEN", 55.449),
            ("APP_API_TOKEN", 49.567),
            ("dapr-api-token", 50.449),
            ("key", 9.443),
        ];
        for (s, want) in cases {
            let got = entropy(s);
            assert!(
                (got - want).abs() < 0.001,
                "{s:?}: got {got}, want {want}"
            );
        }
    }
}
