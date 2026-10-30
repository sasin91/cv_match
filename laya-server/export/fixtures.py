"""Parity fixtures for laya-server's tests: laya-serve's PyTorch path (Router.predict) and
ONNXAgent on the exported graph, plus the exact graph inputs and raw outputs per question.
Synthetic Recruiter-style summaries only, no personal data.

    python fixtures.py <checkpoint dir> <out/laya.onnx> <tests/fixtures.json>"""
import json
import sys

import numpy as np

from laya.onnx_agent import ONNXAgent
from laya.router import Router

CKPT, ONNX, OUT = sys.argv[1], sys.argv[2], sys.argv[3]

QUESTIONS = {
    "meets": {"type": "noul", "instructions": "Does the candidate meet every required qualification for the job?"},
    "fit": {"type": "score", "instructions": "How strong a fit is this candidate for the job?",
            "criteria": ["no fit", "weak fit", "partial fit", "good fit", "excellent fit"]},
}

POOL = ["PHP 8", "Laravel", "MySQL", "Docker", "Kubernetes", "Vue.js", "TypeScript", "REST API design",
        "CI/CD", "AWS", "Redis", "Linux", "Git", "Unit testing", "Danish (fluent)", "English (fluent)",
        "Driver's licence (B)", "Bachelor's degree in computer science", "5+ years of experience",
        "Team leadership", "Agile/Scrum", "GDPR knowledge", "Python", "Go", "Rust", "Elasticsearch",
        "RabbitMQ", "Terraform", "Azure", "GraphQL"]


def summary(title, met, n, missing):
    # Same string Recruiter builds in modules/cv_match/Cv_matcher.php.
    return ("Job: %s. The candidate meets %d of %d required qualifications. Missing requirements: %s."
            % (title, met, n, ", ".join(missing) if missing else "none"))


def miss(n, met, partly=()):
    items = POOL[:n - met] if n - met <= len(POOL) else ["Requirement %d" % i for i in range(1, n - met + 1)]
    return [m + " (partly)" if i in partly else m for i, m in enumerate(items)]


STATES = [
    summary("Senior PHP Developer", 12, 12, []),
    summary("Backend-udvikler", 0, 1, miss(1, 0)),
    summary("Backend-udvikler", 1, 1, []),
    summary("Full Stack Developer", 3, 5, miss(5, 3, partly={1})),
    summary("Senior PHP Developer", 6, 12, miss(12, 6, partly={0, 3})),
    summary("Frontend-udvikler med fokus på Vue", 2, 8, miss(8, 2)),
    summary("DevOps Engineer", 9, 10, miss(10, 9, partly={0})),
    summary("Systemudvikler til Æblegård & Søn ApS, Århus (ø-afdeling)", 4, 7,
            ["Kørekort til lastbil (C)", "Erfaring med Ærø-færgens bookingsystem (partly)", "Flydende færøsk"]),
    summary("Junior Developer", 0, 12, miss(12, 0, partly={2, 5, 9})),
    summary("Data Engineer", 11, 12, miss(12, 11)),
    summary("Lead Software Architect", 0, 2, miss(2, 0, partly={0, 1})),
    summary("Konsulent, offentlig digitalisering", 0, 60,
            ["Requirement %d: experience with system integration area %d in a regulated public-sector environment%s"
             % (i, i, " (partly)" if i % 7 == 0 else "") for i in range(1, 61)]),
]


class Recorder:
    """Wraps the ONNX Runtime session to keep every feed and output the agent sends through it."""

    def __init__(self, session):
        self.session, self.calls = session, []

    def run(self, names, feed, *a, **k):
        out = self.session.run(names, feed, *a, **k)
        self.calls.append(({n: np.asarray(v) for n, v in feed.items()}, dict(zip(names, out))))
        return out

    def __getattr__(self, name):
        return getattr(self.session, name)


router = Router(models={"english": CKPT}, device="cpu", default="english")
onnx_agent = ONNXAgent(CKPT, onnx_path=ONNX)
onnx_agent.session = rec = Recorder(onnx_agent.session)

fixtures, diffs = [], {"noul": 0.0, "score": 0.0, "fit_probabilities": 0.0, "fit_confidence": 0.0, "act_probability": 0.0}
for i, state in enumerate(STATES):
    torch_res = router.predict(state, QUESTIONS, model="english")
    rec.calls.clear()
    onnx_res = onnx_agent.system_one(state, QUESTIONS)
    assert len(rec.calls) == 1, "expected one session run per state, got %d" % len(rec.calls)
    feed, outs = rec.calls[0]
    graph = {}
    for row, qid in enumerate(QUESTIONS):  # rows follow question order
        mask = feed["marker_mask"][row].astype(bool)
        L = int(feed["attention_mask"][row].sum())
        graph[qid] = {
            "input_ids": feed["input_ids"][row, :L].tolist(),
            "attention_mask": feed["attention_mask"][row, :L].tolist(),
            "padded_len": int(feed["input_ids"].shape[1]),
            "marker_pos": feed["marker_pos"][row][mask].tolist(),
            "qtype": int(feed["qtype"][row]),
            "logits": outs["logits"][row][mask].tolist(),
            "act_logits": outs["act_logits"][row].tolist(),
        }
    ta, oa = torch_res["answers"], onnx_res["answers"]
    diffs["noul"] = max(diffs["noul"], abs(ta["meets"]["noul"] - oa["meets"]["noul"]))
    diffs["score"] = max(diffs["score"], abs(ta["fit"]["score"] - oa["fit"]["score"]))
    diffs["fit_confidence"] = max(diffs["fit_confidence"], abs(ta["fit"]["confidence"] - oa["fit"]["confidence"]))
    diffs["fit_probabilities"] = max(diffs["fit_probabilities"], *(abs(ta["fit"]["probabilities"][k] - oa["fit"]["probabilities"][k])
                                                                   for k in ta["fit"]["probabilities"]))
    for q in QUESTIONS:
        diffs["act_probability"] = max(diffs["act_probability"],
                                       abs(ta[q]["action"]["act_probability"] - oa[q]["action"]["act_probability"]))

    def walk(a, b, path):  # every numeric leaf both answers share, plus any key present in only one
        if isinstance(a, dict) and isinstance(b, dict):
            for k in set(a) | set(b):
                if k not in a or k not in b:
                    diffs.setdefault("only_in_" + ("torch" if k in a else "onnx"), set()).add(path + "." + k)
                else:
                    walk(a[k], b[k], path + "." + k)
        elif isinstance(a, (int, float)) and isinstance(b, (int, float)) and not isinstance(a, bool):
            key = "leaf:" + path.split(".", 2)[-1]
            diffs[key] = max(diffs.get(key, 0.0), abs(a - b))
        elif a != b:
            diffs.setdefault("value_mismatch", set()).add(path)
    walk(ta, oa, "answers")
    fixtures.append({"id": i, "state": state, "torch": torch_res, "onnx": onnx_res, "graph": graph})
    print("%2d len=%3d meets t=%.4f o=%.4f | fit t=%.4f o=%.4f" % (
        i, len(graph["meets"]["input_ids"]), ta["meets"]["noul"], oa["meets"]["noul"], ta["fit"]["score"], oa["fit"]["score"]))

diffs = {k: sorted(v) if isinstance(v, set) else v for k, v in diffs.items()}
with open(OUT, "w", encoding="utf-8") as f:
    json.dump({"checkpoint": "convaiinnovations/laya@7b928d828b7b0e022f929d9bd2e44165aa270148 (english, repo root)",
               "laya_version": __import__("laya").__version__, "model": "english", "questions": QUESTIONS,
               "max_abs_diff_torch_vs_onnx": diffs, "states": fixtures}, f, ensure_ascii=False, indent=1)
print("MAXDIFF", json.dumps(diffs))
