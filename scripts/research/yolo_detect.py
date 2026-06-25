#!/usr/bin/env python3
"""Tier4 검증: 일반 객체검출(YOLOv8n, COCO 80클래스) — '특정 피사체 포함 컷만' 컬링.
ultralytics는 .pt를 ONNX로 export 가능 → RawBlow의 ort 런타임에 그대로 올릴 수 있다.
사용: python yolo_detect.py <image_dir>
"""
import sys, glob, os
from collections import Counter
from ultralytics import YOLO

model = YOLO("yolov8n.pt")  # 최초 1회 ~6MB 자동 다운로드
print(f"{'image':<15} objects (class:count, conf>=0.35)")
print("-" * 60)
for p in sorted(glob.glob(os.path.join(sys.argv[1], "*.jpg"))):
    r = model(p, verbose=False, conf=0.35)[0]
    names = r.names
    cnt = Counter(names[int(b.cls)] for b in r.boxes)
    summary = ", ".join(f"{k}:{v}" for k, v in cnt.most_common()) or "(none)"
    print(f"{os.path.basename(p):<15} {summary}")
