#!/usr/bin/env python3
"""Tier3b 검증: CLIP 임베딩으로 임의 프롬프트 축 점수 + 의미적 유사도(클러스터링).
사용: python clip_axes.py <image_dir>
증명: 인물/풍경 장르, 선명도, 구도, 다중 장면분류, 이미지간 코사인 유사도가
      CLIP 이미지 임베딩 · 텍스트 임베딩만으로 (재export 없이) 나온다.
"""
import sys, glob, os, itertools
import torch, open_clip
from PIL import Image

dev = "cpu"
model, _, preprocess = open_clip.create_model_and_transforms("ViT-B-32", pretrained="openai")
model.eval().to(dev)
tok = open_clip.get_tokenizer("ViT-B-32")
ls = model.logit_scale.exp().item()

def temb(prompts):
    with torch.no_grad():
        t = model.encode_text(tok(prompts).to(dev))
        return t / t.norm(dim=-1, keepdim=True)

# (pos, neg) 안토님 프롬프트쌍 → score = P(pos)
AXES = {
    "quality":   ("a high quality photo", "a low quality photo"),
    "sharp":     ("a sharp photo", "a blurry out of focus photo"),
    "portrait":  ("a portrait of a person", "a landscape scenery without people"),
    "compose":   ("a well composed photo", "a poorly composed snapshot"),
}
axis_emb = {k: temb([p, n]) for k, (p, n) in AXES.items()}

SCENES = ["a portrait of a person", "a landscape", "an animal", "food",
          "architecture or a building", "a street or sports scene"]
scene_emb = temb(SCENES)

paths = sorted(glob.glob(os.path.join(sys.argv[1], "*.jpg")))
embs = {}
hdr = f"{'image':<15}" + "".join(f"{k:>9}" for k in AXES) + "   top-scene"
print(hdr); print("-" * len(hdr))
for p in paths:
    try:
        im = preprocess(Image.open(p).convert("RGB")).unsqueeze(0).to(dev)
    except Exception as e:
        print(f"{os.path.basename(p):<15} skip: {e}"); continue
    with torch.no_grad():
        f = model.encode_image(im); f = f / f.norm(dim=-1, keepdim=True)
    embs[os.path.basename(p)] = f
    cells = []
    for k in AXES:
        p_pos = (ls * f @ axis_emb[k].t()).softmax(-1)[0, 0].item()
        cells.append(f"{p_pos:9.2f}")
    sc = (ls * f @ scene_emb.t()).softmax(-1)[0]
    scene = f"{SCENES[int(sc.argmax())]} ({sc.max():.2f})"
    print(f"{os.path.basename(p):<15}" + "".join(cells) + f"   {scene}")

print("\n=== 이미지간 코사인 유사도(>0.80 = 의미적 근접/중복 후보) ===")
ns = list(embs)
any_pair = False
for a, b in itertools.combinations(ns, 2):
    cos = (embs[a] @ embs[b].t()).item()
    if cos > 0.80:
        print(f"  {a} ~ {b}: {cos:.3f}"); any_pair = True
if not any_pair:
    print("  (0.80 초과 쌍 없음 - 전부 시각적으로 충분히 다름)")
