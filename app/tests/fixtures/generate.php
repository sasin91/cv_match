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
require_once "$root/modules/cv_match/Apply_instructions.php";
require_once "$root/modules/cv_match/Joblog.php";

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

// ---------- Apply_instructions ----------

$apply = [];
foreach ([...array_values($posts), ...array_values($cvs),
    "Send din ansøgning og CV til job@firma.dk. Skriv \"Backend 2026\" i emnefeltet.",
    "Mærk din ansøgning \"Udvikler\" og send den til hr@kommune.dk senest 1. november.",
    "To apply, email careers@acme.com with the subject 'Purple Elephant' and answer these questions:\n1. What did you build last?\n2. Why us?\n\nWe look forward to hearing from you.",
    "Ansøgninger via LinkedIn Easy Apply tages ikke i betragtning. Send en mail til rekruttering@firma.dk og besvar følgende spørgsmål:\n- Hvorfor os?\n- Hvad er dit yndlingsværktøj?",
    "Please start your cover letter with the word 'banana' so we know you read the whole post.",
    "Send en mail til job@firma.dk med emnet \"Backend 2026\".",
    "Send din ansøgning og CV til job@firma.dk.",
    "Har du spørgsmål til stillingen, så kontakt Peter på peter@firma.dk eller 12345678.",
    "If you have any questions, email jane@acme.com.",
    "Vil du vide mere, så skriv til hr@firma.dk.",
    "Klik på Søg stillingen for at ansøge. Ansøgningsfrist 1. november.",
    "You know Mailchimp, email marketing and our CV parser.",
    "Du skal svare på mails fra kunder.",
    "Om jobbet\n\nSend en mail til job@firma.dk og besvar følgende spørgsmål:\n\n- Hvorfor os?\n- Hvad er dit yndlingsværktøj?\n\nVi holder samtaler løbende.",
    "Send your CV to jobs@firma.dk.\r\nWrite ‘Rust rocks’ at the top of your application. Applications without it will not be considered.",
    "Ansøg på mail: Skriv «Frontend» i emnelinjen og vedhæft CV. Svar på disse spørgsmål:\n• Hvad har du bygget?\n• Hvorfor os?",
] as $text) {
    $apply[] = ['text' => $text, 'found' => Apply_instructions::read($text)];
}

// ---------- Joblog ----------

// Texts with the deadlines, hours, places and contact people the joblog reads, and the posts.
$joblog_texts = [...array_values($posts),
    'Ansøgningsfrist: 31. oktober 2026',
    'Søg senest den 2.11.',
    'Deadline: November 5th, 2026',
    'Sidste ansøgningsdag er 2026-11-30 kl. 12.00',
    'Ansøgningsfrist 15/1, vi holder samtaler løbende',
    'Ansøgningsfrist: Løbende',
    'Frist: snarest. Start 1. januar',
    'Tiltrædelse 1. december 2026',
    'Ansøgningsfrist 1. november 2026 x' . str_repeat('ø', 60),
    'Send din ansøgning senest d. 3/12-26. Samtaler afholdes i uge 50.',
    'Applications by 1 Dec. We read them as they come.',
    'Ansøgningsfrist: 29. februar 2027',
    'Apply before 31/04/2027 or 30/4/2027',
    'Frist 12. sept., kl. 12.00',
    'Closing date: 2026-13-01, last day to apply 7 jan',
    'Fuldtid, 37 timer om ugen',
    'Deltid 25 timer/uge',
    'Part-time (20 hours per week)',
    'Deltid',
    'Fuldtid eller deltid',
    'Ugentlig arbejdstid: 32,5 timer',
    'Stillingen er på 15-20 timer om ugen',
    'Weekly hours: 40 hours',
    "Backend-udvikler\nAcme ApS\nVestergade 12, 2. th., 8000 Aarhus C",
    'Adresse: Lyngbyvej 2, 2100 København Ø',
    'Acme A/S, DK-2800 Kgs. Lyngby',
    "Arbejdssted: Odense",
    "Softwareudvikler\nTV 2 Danmark\nOdense, Region Syddanmark, Danmark\nfor 2 uger siden",
    "Aarhus C, Denmark",
    "Vi samarbejder med kunder i hele Danmark",
    'Pr. 1. januar 2027 Vi tilbyder 5000 kr. om måneden',
    'Kontoret ligger i Slagelse, tæt på stationen.',
    'Office in Copenhagen, hybrid',
    'Job location: Kongens Lyngby',
    "Adresse: Nørregade 7B, st. tv, 4200 Slagelse",
    'Har du spørgsmål til stillingen, er du velkommen til at kontakte afdelingsleder Mette Hansen på tlf. 12 34 56 78 eller mette.hansen@acme.dk.',
    "Vil du vide mere?\nKontakt CTO Peter Olsen\n+45 22 33 44 55",
    'CVR 12345678. Send din ansøgning til Acme ApS.',
    "Spørgsmål? Ring til Anne-Marie Lund eller Søren B. Kristensen på 87 65 43 21.\nSøren svarer på soeren.kristensen@acme.dk",
    "If you have questions, contact Head of Engineering Jane Doe (jane.doe@acme.com, phone +45 11223344).",
    "Kontakt Acme ApS på 70 20 30 40",
    "For mere information kontakt HR på hr@acme.dk",
];
$saved_at = gmmktime(12, 0, 0, 10, 19, 2026);
$joblog = ['deadlines' => [], 'hours' => [], 'places' => [], 'contacts' => [], 'entries' => [], 'applied_on' => []];
foreach ($joblog_texts as $text) {
    foreach ([$saved_at, gmmktime(12, 0, 0, 12, 20, 2026), gmmktime(12, 0, 0, 2, 1, 2027)] as $at) {
        $joblog['deadlines'][] = ['text' => $text, 'saved_at' => $at, 'deadline' => Joblog::deadline($text, $at)];
    }
    $joblog['hours'][] = ['text' => $text, 'hours' => Joblog::working_hours($text)];
    $joblog['places'][] = ['text' => $text, 'place' => Joblog::place($text)];
    foreach (['Acme ApS', ''] as $company) {
        $joblog['contacts'][] = ['text' => $text, 'company' => $company,
            'contact' => Joblog::contact($text, $company, Apply_instructions::read($text))];
    }
}
// Saved matches as Cv_match_model::find gives them, with each kind of joblog status.
$integrations = [
    null,
    ['status' => 'IkkeSoegt', 'applied_at' => null, 'logged_at' => null],
    ['status' => 'Soegt', 'applied_at' => gmmktime(23, 30, 0, 10, 23, 2026), 'logged_at' => null],
    ['status' => 'Samtale', 'applied_at' => gmmktime(10, 0, 0, 7, 1, 2026), 'logged_at' => 1792400000],
];
$items = [
    ['criterion' => 'title', 'text' => 'Backend-udvikler', 'verdict' => 'met'],
    ['criterion' => 'requirements', 'text' => 'PHP', 'verdict' => 'met'],
    ['criterion' => 'requirements', 'text' => 'Go', 'verdict' => 'partial'],
    ['criterion' => 'requirements', 'text' => 'Kubernetes', 'verdict' => 'missing'],
];
$i = 0;
foreach ([...array_values($posts), ...array_slice($joblog_texts, -8), "Ansøgningen sendes til job@firma.dk med emnet \"Backend\".\r\nArbejdssted:\u{a0}Odense"] as $text) {
    $match = [
        'job_title' => " Job $i ",
        'company' => $i % 2 ? 'Acme ApS' : '',
        'job_url' => $i % 3 ? "https://example.dk/job/$i" : null,
        'job_text' => $text,
        'cv_text' => $i % 4 ? 'Jonas Hansen' : '',
        'created_at' => $saved_at,
        'score' => 0.8234 - $i / 100,
        'tag' => $i % 5 ? 'good' : '',
        'items' => $i % 2 ? $items : array_fill(0, 120, ['criterion' => 'requirements', 'text' => "Krav nummer $i med en lang tekst", 'verdict' => 'met']),
        'application_text' => $i % 3 ? 'Kære Acme' : null,
        'resume_text' => $i % 2 ? '' : 'Tailored',
        'jobnet_joblog' => $integrations[$i % count($integrations)],
    ];
    $joblog['entries'][] = ['match' => $match, 'entry' => Joblog::read($match + ['apply_instructions' => Apply_instructions::read($text)])];
    $i++;
}
foreach ([gmmktime(0, 59, 0, 3, 29, 2026), gmmktime(1, 0, 0, 3, 29, 2026), gmmktime(22, 59, 0, 10, 24, 2026),
          gmmktime(23, 0, 0, 10, 25, 2026), gmmktime(22, 30, 0, 12, 31, 2026)] as $at) {
    foreach (['Soegt', 'IkkeSoegt'] as $status) {
        $joblog['applied_on'][] = ['status' => $status, 'applied_at' => $at,
            'applied_on' => Joblog::applied_on(['status' => $status, 'applied_at' => $at])];
    }
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
    'apply_instructions' => $apply,
    'joblog' => $joblog,
];
file_put_contents(__DIR__ . '/parity.json', json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR) . "\n");
echo count($rank), " rankings, ", count($readings), " readings, ", count($matches), " matches\n";
