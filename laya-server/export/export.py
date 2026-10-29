"""Export the English Laya checkpoint to ONNX (fp32) for laya-server.

Mirrors the settings of scripts/export_onnx.py in NandhaKishorM/laya (Apache-2.0): the same
input/output names, dynamic axes, batch-2 trace inputs, opset 18 and constant folding, and the
same batch 1/2/3 check against PyTorch in probability space (atol 1e-3).

    python export.py <checkpoint dir> <out/laya.onnx>

torch writes the weights (1.7 GB) to laya.onnx.data beside laya.onnx; keep the two together."""
import os
import sys

import numpy as np
import onnx
import onnxruntime as ort
import torch

from laya.agent import Agent
from laya.common import QTYPES

CKPT, OUT = sys.argv[1], sys.argv[2]

INPUT_NAMES = ["input_ids", "attention_mask", "marker_pos", "marker_mask", "qtype"]
OUTPUT_NAMES = ["logits", "act_logits"]
DYNAMIC_AXES = {
    "input_ids": {0: "batch_size", 1: "seq_len"},
    "attention_mask": {0: "batch_size", 1: "seq_len"},
    "marker_pos": {0: "batch_size", 1: "num_markers"},
    "marker_mask": {0: "batch_size", 1: "num_markers"},
    "qtype": {0: "batch_size"},
    "logits": {0: "batch_size", 1: "num_markers"},
    "act_logits": {0: "batch_size"},
}


def example_inputs(batch=2, seq_len=16, num_markers=2):
    g = torch.Generator().manual_seed(0)
    input_ids = torch.randint(0, 100, (batch, seq_len), generator=g, dtype=torch.long)
    attention_mask = torch.ones((batch, seq_len), dtype=torch.long)
    marker_pos = torch.arange(1, num_markers + 1, dtype=torch.long).repeat(batch, 1)
    marker_mask = torch.ones((batch, num_markers), dtype=torch.bool)
    for row in range(1, batch):
        attention_mask[row, max(1, seq_len - row):] = 0
        marker_mask[row, -1] = False
    qtype = torch.arange(batch, dtype=torch.long) % len(QTYPES)
    return input_ids, attention_mask, marker_pos, marker_mask, qtype


def softmax(x):
    e = np.exp(x - x.max(-1, keepdims=True))
    return e / e.sum(-1, keepdims=True)


agent = Agent(CKPT, compile=False, device="cpu")
model = agent.model.eval()
os.makedirs(os.path.dirname(os.path.abspath(OUT)), exist_ok=True)
with torch.no_grad():
    torch.onnx.export(model, example_inputs(), OUT, export_params=True, opset_version=18,
                      do_constant_folding=True, input_names=INPUT_NAMES, output_names=OUTPUT_NAMES,
                      dynamic_axes=DYNAMIC_AXES)
print("exported", OUT)

m = onnx.load(OUT, load_external_data=False)
print("graph inputs:", [(i.name, i.type.tensor_type.elem_type,
                         [d.dim_param or d.dim_value for d in i.type.tensor_type.shape.dim]) for i in m.graph.input])
print("graph outputs:", [(o.name, [d.dim_param or d.dim_value for d in o.type.tensor_type.shape.dim]) for o in m.graph.output])
print("opset:", [(o.domain, o.version) for o in m.opset_import])

sess = ort.InferenceSession(OUT, providers=["CPUExecutionProvider"])
ok = True
for batch in (1, 2, 3):
    inputs = example_inputs(batch=batch, seq_len=16 + 8 * batch, num_markers=1 + batch)
    with torch.no_grad():
        want = model(*inputs)
    got = sess.run(OUTPUT_NAMES, {n: t.numpy() for n, t in zip(INPUT_NAMES, inputs)})
    for name, a, w in zip(OUTPUT_NAMES, got, want):
        w = w.numpy()
        if a.shape != w.shape:
            print("  batch %d %s SHAPE MISMATCH %s vs %s" % (batch, name, a.shape, w.shape)); ok = False; continue
        d = float(np.max(np.abs(softmax(a) - softmax(w))))
        ok &= d <= 1e-3
        print("  batch %-2d %-10s max abs prob diff %.2e (raw logits %.2e) %s"
              % (batch, name, d, float(np.max(np.abs(a - w))), "ok" if d <= 1e-3 else "FAIL"))
print("VERIFY", "PASSED" if ok else "FAILED")
