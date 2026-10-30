<?php
/**
 * Writes the parity fixtures the Rust port is tested against: what the PHP
 * taxonomy lookup, Free_reader and Cv_matcher answer for the same inputs.
 *
 *   php desktop/recruiter/tests/fixtures/generate.php
 *
 * Run it again whenever the PHP side changes; the Rust tests then show what
 * the port has to follow. Inputs are the skill names in eval/data/gold.jsonl
 * (public job ads) and the synthetic posts and CVs below.
 */
$root = dirname(__DIR__, 4);
require_once "$root/taxonomy/php/Taxonomy.php";
require_once "$root/modules/cv_match/Free_reader.php";
require_once "$root/modules/cv_match/Cv_matcher.php";
require_once "$root/modules/cv_match/Match_score.php";

// The day years_worked() counts "until now" from, fixed so the fixtures don't age.
const NOW = '2026-10-30 12:00:00 UTC';

$taxonomy = Taxonomy::load();

/** A ranked result without the term's prepared fields, which the Rust side builds itself. */
function result_row(array $result, string $phrase): array {
    return [
        'id' => $result['term']['id'],
        'score' => $result['score'],
        'lexical' => $result['lexical'],
        'matched' => $result['matched'],
        'method' => Taxonomy::match_method($result, $phrase),
    ];
}

/** Free_reader::cv()'s experience_years, counted on `now` instead of today. */
function cv_years(string $text, int $now): int {
    $call = fn(string $method, ...$args) => (new ReflectionMethod(Free_reader::class, $method))->invoke(null, ...$args);
    $lines = $call('lines', $text);
    $education = $call('section', $lines, (new ReflectionClassConstant(Free_reader::class, 'EDUCATION'))->getValue(),
        (new ReflectionClassConstant(Free_reader::class, 'WORK'))->getValue());
    return Free_reader::years_worked(implode("\n", array_values(array_diff($lines, $education))), $now);
}

// ---------- the taxonomy lookup ----------

$phrases = [];
foreach (file("$root/eval/data/gold.jsonl") as $line) {
    $ad = json_decode($line, true);
    $phrases[] = $ad['jobtitle'];
    foreach ($ad['skills'] as $skill) {
        $phrases[] = $skill['name'];
    }
    foreach ($ad['certifications'] as $name) {
        $phrases[] = $name;
    }
}
// Typos, qualifiers, case and decomposed letters, as job ads and CVs write them.
array_push($phrases, 'projektleeder', 'Erfaring med SAP', 'strong knowledge of Python', 'kørekort B',
    'Kubernets', 'javascrpt', 'a' . "\u{030A}" . 'rsag', 'IT-support', 'C#', 'Laravel', 'php', 'React.js',
    'Social- og sundhedsassistent', '', '   ', '2024', 'Microsoft Office 365');
$phrases = array_values(array_unique($phrases));

$rank = [];
foreach ($phrases as $phrase) {
    foreach ([null, 'skill', 'title', 'role'] as $kind) {
        $rank[] = [
            'phrase' => $phrase,
            'kind' => $kind,
            'results' => array_map(fn(array $r) => result_row($r, $phrase), $taxonomy->rank($phrase, null, $kind, 3)),
        ];
    }
}

$words = [];
foreach ($phrases as $phrase) {
    $words[] = ['text' => $phrase, 'words' => Taxonomy::words($phrase), 'content' => Taxonomy::content_words($phrase)];
}

$distances = [];
foreach ([['kubernetes', 'kubernets'], ['ab', 'ba'], ['projektleder', 'projektleeder'], ['abc', 'abcdef'],
          ['kørekort', 'kørekrot'], ['', 'a'], ['sap', 'sap'], ['ledelse', 'lederskab']] as [$a, $b]) {
    foreach ([1, 2, 3] as $max) {
        $distances[] = ['a' => $a, 'b' => $b, 'max' => $max, 'distance' => Taxonomy::edit_distance($a, $b, $max)];
    }
}

// ---------- Free_reader and Cv_matcher ----------

$posts = require __DIR__ . '/posts.php';
$cvs = require __DIR__ . '/cvs.php';
$now = strtotime(NOW);

$readings = [];
foreach ($posts as $name => $text) {
    $reader = new Free_reader($taxonomy);
    $readings[] = ['name' => $name, 'kind' => 'job', 'text' => $text, 'answer' => $reader->job($text), 'known' => array_keys($reader->known)];
}
foreach ($cvs as $name => $text) {
    $reader = new Free_reader($taxonomy);
    $answer = $reader->cv($text);
    // cv() counts years up to today, so count them again as it does on the fixed day.
    $answer['experience_years'] = cv_years($text, $now);
    $readings[] = ['name' => $name, 'kind' => 'cv', 'text' => $text, 'answer' => $answer, 'known' => array_keys($reader->known)];
}

$years = [];
foreach ($cvs as $name => $text) {
    $years[] = ['name' => $name, 'years' => Free_reader::years_worked($text, $now)];
}
foreach (['September 2024 – February 2026', '2017 - nu', '03/2019 - 08/2021', 'jan. 2020 til dec. 2020, maj 2020 - 2022',
          '2030 - 2031', '2010-2005', 'Okt 2015 – i dag'] as $text) {
    $years[] = ['name' => $text, 'text' => $text, 'years' => Free_reader::years_worked($text, $now)];
}

$asked = [];
foreach (['3+ års erfaring med PHP', 'flere års erfaring', '5 years of experience', 'a few years of experience',
          '2-4 års erfaring', 'Erfaring med SAP', 'mange års erfaring', 'Experience: 10 yrs', '3 år'] as $line) {
    $asked[] = ['line' => $line, 'years' => Free_reader::years_asked($line)];
}

$matches = [];
foreach ($posts as $post_name => $post) {
    foreach ($cvs as $cv_name => $cv) {
        $reader = new Free_reader($taxonomy);
        $job = $reader->job($post);
        $profile = $reader->cv($cv);
        $profile['experience_years'] = cv_years($cv, $now);
        $matcher = new Cv_matcher($taxonomy, $profile, $reader->known);
        $groups = Cv_matcher::criteria($job);
        $verdicts = [];
        foreach (Cv_matcher::units($groups) as $unit) {
            $verdict = $matcher->decide($unit);
            if ($verdict) {
                $verdicts[$unit['id']] = $verdict;
            }
        }
        $combined = Cv_matcher::combine($groups, $verdicts);
        $matches[] = [
            'post' => $post_name,
            'cv' => $cv_name,
            'job' => $job,
            'profile' => $profile,
            'groups' => $groups,
            'verdicts' => (object) $verdicts,
            'combined' => (object) $combined,
            'score' => Cv_matcher::score($groups, $combined),
            'laya_summary' => Cv_matcher::laya_summary($job, $groups, $combined),
            'soft_skills' => Cv_matcher::soft_skills($job),
        ];
    }
}

$scores = [];
foreach ([0.0, 0.004, 0.005, 0.595, 0.6, 0.79999, 0.8, 0.845, 0.85, 1.0] as $score) {
    $scores[] = ['score' => $score, 'percent' => Match_score::percent($score), 'tag' => Cv_matcher::match_tag($score)];
}

$out = [
    'now' => NOW,
    'rank' => $rank,
    'words' => $words,
    'distances' => $distances,
    'readings' => $readings,
    'years_worked' => $years,
    'years_asked' => $asked,
    'matches' => $matches,
    'scores' => $scores,
];
file_put_contents(__DIR__ . '/parity.json', json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR) . "\n");
echo count($rank), " rankings, ", count($readings), " readings, ", count($matches), " matches\n";
