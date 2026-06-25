#!/usr/bin/env python3
"""CLIP 다축(多軸) 미적/의미 점수 모델을 ONNX로 export (#50, Tier3b).

export_clip_iqa.py(Good/Bad 1축)의 일반화: 여러 안토님 프롬프트쌍을 그래프에 상수로
구워넣어, 입력=이미지(1,3,224,224) → 출력=(N,K) 각 축의 P(positive). 런타임(Rust)은
텍스트 인코더 없이 이미지만 넣으면 K개 축 점수를 한 번에 받는다.

축(K=4): quality / genre_portrait(인물↔풍경) / sharp / compose.
라이선스: OpenAI CLIP(MIT) 가중치만 사용 — 상용 RawBlow에 번들·재배포 가능.

사용: python scripts/export_clip_axes.py --backbone ViT-B-32 --out dist/models [--verify <img_dir>]
"""
import argparse, hashlib, os, sys

# (이름, positive, negative) — col k = P(positive_k)
PAIRS = [
    ("quality", "a high quality photo", "a low quality photo"),
    ("genre_portrait", "a portrait of a person", "a landscape scenery without people"),
    ("sharp", "a sharp in-focus photo", "a blurry out-of-focus photo"),
    ("compose", "a well composed photo", "a poorly composed snapshot"),
]
AXES = [p[0] for p in PAIRS]


def build(backbone):
    import torch, torch.nn as nn, open_clip
    model, _, _ = open_clip.create_model_and_transforms(backbone, pretrained="openai")
    model.eval()
    tok = open_clip.get_tokenizer(backbone)
    prompts = []
    for _, pos, neg in PAIRS:
        prompts += [pos, neg]
    with torch.no_grad():
        t = model.encode_text(tok(prompts))
        t = t / t.norm(dim=-1, keepdim=True)          # (2K, D)
        scale = model.logit_scale.exp().item()
    K = len(PAIRS)

    class ClipAxes(nn.Module):
        def __init__(self):
            super().__init__()
            self.visual = model.visual
            self.register_buffer("text", t)            # (2K, D) 상수
            self.scale = float(scale)
            self.k = K

        def forward(self, image):                      # (N,3,224,224)
            f = self.visual(image)
            f = f / f.norm(dim=-1, keepdim=True)
            logits = self.scale * f @ self.text.t()    # (N, 2K)
            logits = logits.view(-1, self.k, 2)        # (N, K, [pos,neg])
            return logits.softmax(dim=-1)[..., 0]      # (N, K) = P(pos)

    return ClipAxes().eval(), K


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for c in iter(lambda: f.read(1 << 20), b""):
            h.update(c)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--backbone", default="ViT-B-32", choices=["RN50", "ViT-B-32", "ViT-L-14"])
    ap.add_argument("--out", default="dist/models")
    ap.add_argument("--no-quant", action="store_true")
    ap.add_argument("--verify", default=None, help="이미지 폴더로 export된 ONNX 검증")
    args = ap.parse_args()
    import torch
    os.makedirs(args.out, exist_ok=True)
    wrapper, K = build(args.backbone)
    print(f"axes(K={K}): {AXES}")
    fp32 = os.path.join(args.out, f"clip-axes-{args.backbone}.onnx")
    torch.onnx.export(
        wrapper, torch.zeros(1, 3, 224, 224), fp32,
        input_names=["image"], output_names=["axes"],
        opset_version=17, dynamic_axes={"image": {0: "batch"}, "axes": {0: "batch"}},
        dynamo=False,
    )
    print(f"[fp32] {fp32}  {os.path.getsize(fp32)/1e6:.1f} MB  sha256={sha256(fp32)}")
    out_model = fp32
    if not args.no_quant:
        from onnxruntime.quantization import quantize_dynamic, QuantType
        int8 = os.path.join(args.out, f"clip-axes-{args.backbone}.int8.onnx")
        quantize_dynamic(fp32, int8, weight_type=QuantType.QInt8)
        print(f"[int8] {int8}  {os.path.getsize(int8)/1e6:.1f} MB  sha256={sha256(int8)}")
        out_model = int8

    if args.verify:
        import glob, numpy as np, onnxruntime as ort
        from PIL import Image
        MEAN = np.array([0.48145466, 0.4578275, 0.40821073], np.float32)
        STD = np.array([0.26862954, 0.26130258, 0.27577711], np.float32)
        sess = ort.InferenceSession(out_model, providers=["CPUExecutionProvider"])
        print(f"\nverify with {out_model}")
        print("image           " + "".join(f"{a:>16}" for a in AXES))
        for p in sorted(glob.glob(os.path.join(args.verify, "*.jpg"))):
            im = Image.open(p).convert("RGB").resize((224, 224), Image.BICUBIC)
            x = (np.asarray(im, np.float32) / 255.0 - MEAN) / STD
            x = x.transpose(2, 0, 1)[None]
            out = sess.run(None, {"image": x})[0][0]
            print(f"{os.path.basename(p):<15}" + "".join(f"{v:>16.2f}" for v in out))


if __name__ == "__main__":
    main()
