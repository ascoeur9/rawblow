#!/usr/bin/env python3
"""Tier4 검증: YuNet 얼굴검출(cv2.FaceDetectorYN). 얼굴 박스 + 5랜드마크(우/좌 눈,
코, 우/좌 입꼬리) + 신뢰도. 인물 사진엔 얼굴 검출, 풍경엔 0 → '인물 컬링' 가능 증명.
눈 좌표를 알므로 '눈감음' 판정은 그 눈영역 크롭 + 소형 분류기로 이어붙일 수 있다(후속).
사용: python face_detect.py <yunet.onnx> <image_dir>
"""
import sys, glob, os, cv2

model_path, img_dir = sys.argv[1], sys.argv[2]
det = cv2.FaceDetectorYN.create(model_path, "", (320, 320), 0.6, 0.3, 50)

print(f"{'image':<15} {'faces':>5}   detail (box wxh @x,y  eyes  conf)")
print("-" * 78)
for p in sorted(glob.glob(os.path.join(img_dir, "*.jpg"))):
    img = cv2.imread(p)
    if img is None:
        print(f"{os.path.basename(p):<15} (load fail)"); continue
    h, w = img.shape[:2]
    det.setInputSize((w, h))
    _, faces = det.detect(img)
    n = 0 if faces is None else len(faces)
    line = f"{os.path.basename(p):<15} {n:>5}"
    if n:
        f = faces[0]
        bx, by, bw, bh = (int(v) for v in f[:4])
        reye = (int(f[4]), int(f[5]))
        leye = (int(f[6]), int(f[7]))
        conf = float(f[14])
        line += f"   {bw}x{bh} @{bx},{by}  Reye{reye} Leye{leye}  {conf:.2f}"
    print(line)
