#!/usr/bin/env python3
"""CLIP-IQA(제로샷) 미적 점수 모델을 ONNX로 export (#50).

라이선스: OpenAI CLIP 가중치(MIT)만 사용하고 IQA 학습 데이터셋을 쓰지 않는다 →
독점(상업) 앱 RawBlow에 재배포·번들·자동다운로드 가능. 자세한 근거는
memory/rawblow-ai-model-licensing.md 참조.

설계: 모든 걸 ONNX 그래프 하나에 접어 넣는다.
  입력  = 전처리된 이미지 텐서 (1,3,224,224)  [CLIP 정규화는 Rust 쪽에서 수행]
  출력  = (1,2) softmax 확률 [P(좋은 사진), P(나쁜 사진)]
"Good photo." / "Bad photo." 두 프롬프트의 텍스트 특징·logit_scale을 그래프에 상수로
굳혀, 런타임(Rust)은 이미지→2확률만 받으면 된다(텍스트 인코더 불필요).

사용:
  pip install "torch>=2.1" open_clip_torch onnx onnxruntime
  python3 scripts/export_clip_iqa.py --backbone ViT-B-32 --out dist/models
  # 백본: RN50 | ViT-B-32 | ViT-L-14  (앱에서 사용자가 고를 수 있게 셋 다 만들어 둔다)

출력물: <out>/clip-iqa-<backbone>.onnx (fp32) + .int8.onnx (양자화, 배포 권장).
끝에 파일 크기와 sha256을 찍는다 → 앱의 다운로드 매니페스트(URL+해시)에 그대로 사용.
"""
import argparse
import hashlib
import os
import sys

PROMPTS = ["Good photo.", "Bad photo."]
# CLIP-IQA 백본 → open_clip 모델명(전부 OpenAI MIT 가중치).
BACKBONES = {
    "RN50": "RN50",
    "ViT-B-32": "ViT-B-32",
    "ViT-L-14": "ViT-L-14",
}


def build_wrapper(backbone: str):
    import torch
    import torch.nn as nn
    import open_clip

    model, _, _ = open_clip.create_model_and_transforms(backbone, pretrained="openai")
    model.eval()
    tokenizer = open_clip.get_tokenizer(backbone)

    with torch.no_grad():
        tok = tokenizer(PROMPTS)
        text_features = model.encode_text(tok)
        text_features = text_features / text_features.norm(dim=-1, keepdim=True)
        logit_scale = model.logit_scale.exp().item()  # CLIP는 ≈100

    class ClipIqa(nn.Module):
        def __init__(self):
            super().__init__()
            self.visual = model.visual
            self.register_buffer("text_features", text_features)  # (2, D) 상수
            self.logit_scale = float(logit_scale)

        def forward(self, image):  # image: (N,3,224,224), CLIP 정규화 완료
            feats = self.visual(image)
            feats = feats / feats.norm(dim=-1, keepdim=True)
            logits = self.logit_scale * feats @ self.text_features.t()  # (N,2)
            return logits.softmax(dim=-1)  # [P(good), P(bad)]

    return ClipIqa().eval()


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--backbone", choices=list(BACKBONES), default="ViT-B-32")
    ap.add_argument("--out", default="dist/models")
    ap.add_argument("--no-quant", action="store_true", help="int8 양자화 생략")
    args = ap.parse_args()

    import torch

    os.makedirs(args.out, exist_ok=True)
    wrapper = build_wrapper(BACKBONES[args.backbone])
    dummy = torch.zeros(1, 3, 224, 224)

    fp32 = os.path.join(args.out, f"clip-iqa-{args.backbone}.onnx")
    torch.onnx.export(
        wrapper,
        dummy,
        fp32,
        input_names=["image"],
        output_names=["probs"],
        opset_version=17,
        dynamic_axes={"image": {0: "batch"}, "probs": {0: "batch"}},
    )
    print(f"[fp32] {fp32}  {os.path.getsize(fp32)/1e6:.1f} MB  sha256={sha256(fp32)}")

    if not args.no_quant:
        from onnxruntime.quantization import quantize_dynamic, QuantType

        int8 = os.path.join(args.out, f"clip-iqa-{args.backbone}.int8.onnx")
        quantize_dynamic(fp32, int8, weight_type=QuantType.QInt8)
        print(f"[int8] {int8}  {os.path.getsize(int8)/1e6:.1f} MB  sha256={sha256(int8)}")

    # 빠른 자가검증: 회색 이미지 1장을 추론해 출력이 확률(합≈1)인지 확인.
    try:
        import numpy as np
        import onnxruntime as ort

        sess = ort.InferenceSession(fp32, providers=["CPUExecutionProvider"])
        x = np.full((1, 3, 224, 224), 0.0, dtype=np.float32)
        out = sess.run(None, {"image": x})[0]
        print(f"[check] probs={out.ravel().tolist()} sum={float(out.sum()):.3f}")
    except Exception as e:  # noqa: BLE001
        print(f"[check] skipped: {e}", file=sys.stderr)


if __name__ == "__main__":
    main()
