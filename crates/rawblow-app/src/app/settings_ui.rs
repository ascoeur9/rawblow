//! 설정·라이센스 화면(분해 4/8): ui_settings(#22 캐시·#30 언어·#36 배경색 등)·
//! ui_licenses(#39). app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

impl RawBlowApp {
    pub(super) fn ui_settings(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        egui::TopBottomPanel::top("settings_top")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button(format!("← {}", tr(lang, "돌아가기"))).clicked() {
                        self.show_settings = false;
                        let _ = config::save(&self.cfg);
                        self.schedule_cache_trim(); // 변경된 상한으로 캐시 정리.
                    }
                    ui.label(egui::RichText::new("Settings — Keyboard & General").font(prop(14.0)).color(theme::INK));
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(egui::RichText::new("GENERAL").font(prop(11.0)).color(theme::INK3));
                    ui.checkbox(&mut self.cfg.auto_advance, tr(lang, "라벨링 후 자동 전진"));
                    ui.checkbox(&mut self.cfg.recursive, tr(lang, "하위 폴더 포함 스캔"));
                    ui.checkbox(&mut self.cfg.show_exif, tr(lang, "EXIF 오버레이 기본 표시"));
                    ui.checkbox(&mut self.cfg.show_histogram, tr(lang, "히스토그램 기본 표시"));
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "프리로드 ±"));
                        ui.add(egui::DragValue::new(&mut self.cfg.preload).range(0..=10));
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "그리드 열 수"));
                        ui.add(egui::DragValue::new(&mut self.cfg.grid_cols).range(4..=12));
                    });
                    // 스트립·그리드 표기 크기(#44): 셀 위 선택 표시·별점·색상 태그를 크게(기본)/작게.
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "스트립·그리드 표기 크기"));
                        if ui.selectable_label(self.cfg.large_badges, tr(lang, "크게")).clicked() {
                            self.cfg.large_badges = true;
                            let _ = config::save(&self.cfg);
                        }
                        if ui.selectable_label(!self.cfg.large_badges, tr(lang, "작게")).clicked() {
                            self.cfg.large_badges = false;
                            let _ = config::save(&self.cfg);
                        }
                    });
                    // 언어 선택(#30): 시스템(자동)/한국어/English/日本語. 변경 즉시 적용·저장.
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "언어"));
                        let opts: [(Option<Lang>, &str); 4] = [
                            (None, tr(lang, "시스템 (자동)")),
                            (Some(Lang::Ko), Lang::Ko.native_name()),
                            (Some(Lang::En), Lang::En.native_name()),
                            (Some(Lang::Ja), Lang::Ja.native_name()),
                        ];
                        let mut sel = self.cfg.lang;
                        for (val, label) in opts {
                            if ui.selectable_label(sel == val, label).clicked() {
                                sel = val;
                            }
                        }
                        if sel != self.cfg.lang {
                            self.cfg.lang = sel;
                            self.lang = crate::i18n::effective_lang(&self.cfg);
                            // 폰트도 새 언어의 폰트를 primary로 교체(#32 후속: 세로 어긋남 방지).
                            crate::fonts::install(ui.ctx(), self.lang);
                            let _ = config::save(&self.cfg);
                        }
                    });
                    // ── PHOTO BACKGROUND (#36): 사진 표시 화면 배경색 — 프리셋 + HEX/RGB ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("PHOTO BACKGROUND").font(prop(11.0)).color(theme::INK3));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(tr(lang, "사진 표시 화면 배경색 — 프리셋 또는 HEX/RGB로 지정(Lightroom Develop 기본값은 50% 회색)")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(6.0);
                    // 프리셋: (라벨, Option<rgb>) — None은 앱 기본(near-black void).
                    let presets: [(&str, Option<[u8; 3]>); 6] = [
                        (tr(lang, "기본"), None),
                        (tr(lang, "검정"), Some([0x00, 0x00, 0x00])),
                        (tr(lang, "다크 그레이"), Some([0x1e, 0x1e, 0x1e])),
                        (tr(lang, "중간 회색"), Some([0x80, 0x80, 0x80])),
                        (tr(lang, "라이트 그레이"), Some([0xb3, 0xb3, 0xb3])),
                        (tr(lang, "흰색"), Some([0xff, 0xff, 0xff])),
                    ];
                    // 고정폭 셀 그리드: 라벨 길이가 달라도(검정/라이트 그레이/ミディアムグレー) 색견본과
                    // 글자가 같은 열에 맞도록 각 프리셋을 동일 크기 셀에 가운데 정렬한다(테스트 피드백).
                    const BG_CELL: Vec2 = Vec2::new(78.0, 50.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 8.0);
                        for (label, val) in presets {
                            let selected = self.cfg.photo_bg == val;
                            let swatch_rgb = val.unwrap_or(theme::BG0_RGB);
                            let cell = ui.allocate_ui_with_layout(
                                BG_CELL,
                                Layout::top_down(Align::Center),
                                |ui| {
                                    ui.set_width(BG_CELL.x);
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    let clicked = bg_swatch(ui, swatch_rgb, selected);
                                    ui.label(
                                        egui::RichText::new(label)
                                            .font(mono(9.0))
                                            .color(if selected { theme::INK2 } else { theme::INK4 }),
                                    );
                                    clicked
                                },
                            );
                            if cell.inner {
                                self.cfg.photo_bg = val;
                                self.bg_hex = hex_str(self.photo_bg_rgb());
                                let _ = config::save(&self.cfg);
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        // 현재 색 미리보기.
                        let cur = self.photo_bg_rgb();
                        let (sr, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                        ui.painter().rect(sr, Rounding::same(4.0), Color32::from_rgb(cur[0], cur[1], cur[2]), Stroke::new(1.0, theme::LINE3));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("HEX").font(mono(10.0)).color(theme::INK3));
                        let resp = ui.add(egui::TextEdit::singleline(&mut self.bg_hex).font(mono(12.0)).desired_width(84.0).hint_text("#101010"));
                        if resp.changed() {
                            if let Some(rgb) = parse_hex_rgb(&self.bg_hex) {
                                self.cfg.photo_bg = Some(rgb);
                                let _ = config::save(&self.cfg);
                            }
                        }
                        ui.add_space(8.0);
                        let mut rgb = self.photo_bg_rgb();
                        let mut changed = false;
                        ui.label(egui::RichText::new("R").font(mono(10.0)).color(theme::INK3));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[0]).range(0..=255)).changed();
                        ui.label(egui::RichText::new("G").font(mono(10.0)).color(theme::INK3));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[1]).range(0..=255)).changed();
                        ui.label(egui::RichText::new("B").font(mono(10.0)).color(theme::INK3));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[2]).range(0..=255)).changed();
                        if changed {
                            self.cfg.photo_bg = Some(rgb);
                            self.bg_hex = hex_str(rgb);
                            let _ = config::save(&self.cfg);
                        }
                    });

                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("LABELS").font(prop(11.0)).color(theme::INK3));
                    let km = &self.cfg.keymap;
                    for (lbl, key) in [(Label::Pick, &km.pick), (Label::Hold, &km.hold), (Label::Reject, &km.reject), (Label::Unrated, &km.clear)] {
                        ui.horizontal(|ui| {
                            ui.label(lbl.name(lang));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                kbd(ui, key);
                            });
                        });
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tr(lang, "단축키 재바인딩 UI는 v1.1 예정 — 현재 기본값 QWER 고정 표시")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr(lang, "별점 1~5 지정 · ` (백틱)으로 해제 — 라벨(QWER)과 독립으로 동시에 매겨집니다")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr(lang, "M = 촬영 위치 미니 지도(GPS 있는 사진) · A = AF 포인트 표시 — 토글 상태는 저장됩니다")).font(mono(10.0)).color(theme::INK4));

                    // ── COLOR TAGS (#27): 색별 커스텀 이름. 비우면 기본 색 이름 표시 ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("COLOR TAGS").font(prop(11.0)).color(theme::INK3));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(tr(lang, "색별 이름을 지정해 보정 방식 등 나만의 분류로 — ⇧1~5로 부여")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(4.0);
                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 6.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cfg.tag_names[i])
                                    .hint_text(tag.default_name(lang))
                                    .desired_width(220.0),
                            );
                        });
                    }

                    // ── CACHE (#22): 썸네일 디스크 캐시 사용량 + 비우기 ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("CACHE").font(prop(11.0)).color(theme::INK3));
                    if self.cache_size.is_none() {
                        self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                    }
                    let size = self.cache_size.unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(trf(lang, "썸네일 캐시 사용량 · {}", &[&fmt_bytes(size)])).font(mono(11.0)).color(theme::INK2));
                        if toggle_btn(ui, tr(lang, "캐시 비우기"), false).clicked() {
                            let _ = cache::clear(&config::cache_dir());
                            self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                            self.toast = Some((tr(lang, "썸네일 캐시를 비웠습니다").into(), Instant::now()));
                        }
                        if toggle_btn(ui, tr(lang, "새로고침"), false).clicked() {
                            self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr(lang, "자동 상한")).font(mono(11.0)).color(theme::INK2));
                        ui.add(egui::DragValue::new(&mut self.cfg.cache_limit_mb).speed(64.0).range(0..=1_048_576).suffix(" MB"));
                        ui.label(egui::RichText::new(tr(lang, "(0 = 무제한)")).font(mono(10.0)).color(theme::INK4));
                    });
                    ui.label(egui::RichText::new(tr(lang, "상한을 넘으면 오래된 썸네일부터 자동 삭제 — 폴더 열 때·설정 변경 시 정리됩니다.")).font(mono(10.0)).color(theme::INK4));
                    ui.label(egui::RichText::new(tr(lang, "폴더를 다시 열어도 재디코딩 없이 즉시 표시됩니다.")).font(mono(10.0)).color(theme::INK4));
                    ui.label(egui::RichText::new(config::cache_dir().to_string_lossy().to_string()).font(mono(9.5)).color(theme::INK4));

                    // ── ABOUT / LINKS (#18): 버전·릴리즈·이슈·제작자·cosly ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("ABOUT").font(prop(11.0)).color(theme::INK3));
                    ui.label(egui::RichText::new(format!("RawBlow v{}", env!("CARGO_PKG_VERSION"))).font(mono(11.0)).color(theme::INK2));
                    ui.add_space(6.0);
                    link_label(ui, tr(lang, "최신 버전 받기 · GitHub Releases"), "https://github.com/ascoeur9/rawblow/releases");
                    link_label(ui, tr(lang, "버그 제보 · GitHub Issues"), "https://github.com/ascoeur9/rawblow/issues");
                    // 오픈소스 라이센스 고지(#39): 포함된 구성요소 목록 + 전문 페이지.
                    ui.add_space(8.0);
                    if toggle_btn(ui, tr(lang, "오픈소스 라이센스"), false).clicked() {
                        self.licenses = Some(crate::licenses::LicensesPage::new());
                    }
                    ui.label(egui::RichText::new(tr(lang, "이 프로그램이 포함한 오픈소스 구성요소 목록과 라이센스 전문")).font(mono(10.0)).color(theme::INK4));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(lang, "만든 사람 · 하레 (Hare)")).font(prop(11.5)).color(theme::INK2));
                    ui.horizontal(|ui| {
                        link_label(ui, "X · @ascoeur9", "https://x.com/ascoeur9");
                        ui.label(egui::RichText::new("·").font(mono(10.0)).color(theme::INK4));
                        link_label(ui, "X · @hare_kig", "https://x.com/hare_kig");
                    });
                    ui.add_space(10.0);
                    link_label(ui, tr(lang, "투네이션으로 후원하기"), "https://toon.at/donate/hare");
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(lang, "마음에 드시나요? 그럼 cosly도 이용해보세요.")).font(prop(11.5)).color(theme::INK2));
                    link_label(ui, "https://cosly.link", "https://cosly.link");
                });
            });
        self.grid_cols = self.cfg.grid_cols.clamp(4, 12);
        self.show_exif = self.cfg.show_exif;
        self.show_hist = self.cfg.show_histogram;
    }

    /// 오픈소스 라이센스 페이지(#39): 좌측 구성요소 목록(검색 가능) + 우측 라이센스 전문.
    /// 설정의 ABOUT에서 열며, 돌아가기/Esc로 설정 화면에 복귀한다.
    pub(super) fn ui_licenses(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let mut close = false;
        let (total, generated) = self
            .licenses
            .as_ref()
            .map(|p| (p.doc.crates.len(), p.doc.generated.clone()))
            .unwrap_or((0, String::new()));
        egui::TopBottomPanel::top("licenses_top")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button(format!("← {}", tr(lang, "돌아가기"))).clicked() {
                        close = true;
                    }
                    ui.label(egui::RichText::new(tr(lang, "오픈소스 라이센스")).font(prop(14.0)).color(theme::INK));
                    ui.label(egui::RichText::new(trf(lang, "{} 구성요소", &[&total.to_string()])).font(mono(10.5)).color(theme::INK3));
                    if !generated.is_empty() {
                        ui.label(egui::RichText::new(format!("· {}", generated)).font(mono(10.5)).color(theme::INK4));
                    }
                });
            });
        let page = self.licenses.as_mut().unwrap();
        egui::SidePanel::left("licenses_list")
            .exact_width(320.0)
            .resizable(false)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::same(10.0)))
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut page.filter)
                        .font(mono(12.0))
                        .hint_text(tr(lang, "검색"))
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let f = page.filter.to_lowercase();
                    for (i, c) in page.doc.crates.iter().enumerate() {
                        if !f.is_empty() && !c.n.to_lowercase().contains(&f) && !c.l.to_lowercase().contains(&f) {
                            continue;
                        }
                        if ui.selectable_label(page.selected == i, format!("{} {}", c.n, c.v)).clicked() {
                            page.selected = i;
                        }
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG2).inner_margin(egui::Margin::same(18.0)))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(tr(lang, "이 프로그램은 아래의 오픈소스 소프트웨어를 포함합니다. LGPL 구성요소(rawloader · imagepipe · multicache)의 소스코드는 각 항목의 저장소 링크에서 구할 수 있습니다."))
                        .font(mono(10.0))
                        .color(theme::INK4),
                );
                ui.add_space(12.0);
                if let Some(c) = page.doc.crates.get(page.selected) {
                    ui.label(egui::RichText::new(format!("{} v{}", c.n, c.v)).font(prop(14.0)).color(theme::INK));
                    ui.label(egui::RichText::new(c.l.as_str()).font(mono(11.0)).color(theme::INK3));
                    if let Some(r) = &c.r {
                        link_label(ui, r, r);
                    }
                    ui.add_space(10.0);
                    // 전문은 항목별 ScrollArea — selected를 id에 섞어 항목 전환 시 스크롤이 맨 위로.
                    egui::ScrollArea::vertical()
                        .id_salt(("license_text", page.selected))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for &ti in &c.t {
                                if let Some(t) = page.doc.texts.get(ti) {
                                    ui.label(egui::RichText::new(t.as_str()).font(mono(10.5)).color(theme::INK2));
                                    ui.add_space(16.0);
                                }
                            }
                        });
                }
            });
        if close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.licenses = None;
        }
    }
}
