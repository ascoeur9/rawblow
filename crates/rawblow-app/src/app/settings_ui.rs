//! 설정·라이센스 화면(분해 4/8): ui_settings(#22 캐시·#30 언어·#36 배경색 등)·
//! ui_licenses(#39). app.rs에서 순수 이동 — 동작 변경 없음.

use super::*;

// 설정 화면 텍스트 대비 상향: 배경 BG1(#0b0d11) 위에서 기존 INK4(≈2.5:1)·INK3(≈4.5:1)는
// 특히 작은 설명 문구가 잘 안 보였다. 이 화면에 한해 잉크 톤을 한 단계 올린다
// — 설명/힌트 INK4→INK3(≈4.5:1, AA 충족), 섹션 헤더 INK3→INK2(≈8.9:1). 전역 theme는 유지.
use crate::theme::INK2 as INK_HEAD; // 섹션 헤더
use crate::theme::INK3 as INK_HELP; // 설명·힌트

impl RawBlowApp {
    pub(super) fn ui_settings(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        // 돌아가기(버튼·Esc 공통)와 기본값 복원은 플래그로 모아 패널을 그린 뒤 한 곳에서 실행한다 —
        // 버튼과 Esc가 서로 다른 동작으로 어긋나지 않게(#69).
        let mut go_back = false;
        let mut do_reset = false;
        egui::TopBottomPanel::top("settings_top")
            .exact_height(TOOLBAR_H)
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::symmetric(12.0, 6.0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui.button(format!("← {}", tr(lang, "돌아가기"))).clicked() {
                        go_back = true;
                    }
                    // #74: 헤더 전용 캡션 키로 전체 제목을 표시한다(영어 UI에서 "Settings"로 축약되던
                    // 문제 복원). 툴바 툴팁 등에서 재사용하는 "설정"(Settings) 키와 별개.
                    ui.label(egui::RichText::new(tr(lang, "설정 — 키보드 · 일반")).font(prop(14.0)).color(theme::INK));
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme::BG1).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(egui::RichText::new(tr(lang, "일반")).font(prop(11.0)).color(INK_HEAD));
                    // 모든 설정 컨트롤은 변경 즉시 저장(#69). config::save는 작은 원자적 JSON 쓰기라
                    // 체크박스/드래그/텍스트 입력마다 저장해도 부담이 적다(DragValue·TextEdit의 .changed()는
                    // 드래그 틱·키 입력마다 발생). '돌아가기' 저장은 최종 catch-all로 남긴다.
                    if ui.checkbox(&mut self.cfg.auto_advance, tr(lang, "라벨링 후 자동 전진")).changed() {
                        let _ = config::save(&self.cfg);
                    }
                    if ui.checkbox(&mut self.cfg.recursive, tr(lang, "하위 폴더 포함 스캔")).changed() {
                        let _ = config::save(&self.cfg);
                    }
                    if ui.checkbox(&mut self.cfg.show_exif, tr(lang, "EXIF 오버레이 기본 표시")).changed() {
                        let _ = config::save(&self.cfg);
                    }
                    if ui.checkbox(&mut self.cfg.show_histogram, tr(lang, "히스토그램 기본 표시")).changed() {
                        let _ = config::save(&self.cfg);
                    }
                    if ui.checkbox(&mut self.cfg.check_updates, tr(lang, "새 버전 자동 확인")).changed() {
                        let _ = config::save(&self.cfg);
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "프리로드 ±"));
                        if ui.add(egui::DragValue::new(&mut self.cfg.preload).range(0..=10)).changed() {
                            let _ = config::save(&self.cfg);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "그리드 열 수"));
                        if ui.add(egui::DragValue::new(&mut self.cfg.grid_cols).range(4..=12)).changed() {
                            let _ = config::save(&self.cfg);
                        }
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
                    // 정렬 기준(#56): 촬영시간순(기본)/파일명순. 변경 즉시 재정렬·저장.
                    // 촬영시간은 EXIF를 백그라운드로 읽은 뒤 반영된다(큰 폴더·NAS에서 수 초 지연 가능).
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "정렬 기준"));
                        if ui.selectable_label(self.cfg.sort == SortOrder::Name, tr(lang, "파일명순")).clicked() {
                            self.set_sort_order(SortOrder::Name);
                        }
                        if ui
                            .selectable_label(self.cfg.sort == SortOrder::CaptureTime, tr(lang, "촬영시간순"))
                            .clicked()
                        {
                            self.set_sort_order(SortOrder::CaptureTime);
                        }
                    });
                    // 사진 이동 시 원본 보기(ORIG) 유지 방식(#87). 기본은 기존 동작 —
                    // 설정을 건드리지 않으면 v0.5.10과 같다.
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "사진 이동 시 원본 보기"));
                        if ui
                            .selectable_label(self.cfg.view_carry == ViewCarry::ZoomOnly, tr(lang, "확대 중일 때만 유지"))
                            .clicked()
                        {
                            self.cfg.view_carry = ViewCarry::ZoomOnly;
                            let _ = config::save(&self.cfg);
                        }
                        if ui
                            .selectable_label(self.cfg.view_carry == ViewCarry::Keep, tr(lang, "현재 보기 상태 유지"))
                            .clicked()
                        {
                            self.cfg.view_carry = ViewCarry::Keep;
                            let _ = config::save(&self.cfg);
                        }
                    });
                    ui.label(egui::RichText::new(tr(lang, "「현재 보기 상태 유지」는 창맞춤에서도 ORIG를 계속 불러옵니다 — 넘김이 느려지고 메모리를 더 씁니다. 새 폴더는 두 방식 모두 프리뷰로 시작합니다.")).font(mono(10.0)).color(INK_HELP));
                    // 전송 dest 기본값(#113). 경로는 항상 보여 지정 폴더를 바로 고르게 한다.
                    // Cmd+E에서 바꿔도 다음 열기는 다시 이 값.
                    ui.add_space(10.0);
                    ui.label(tr(lang, "전송 폴더 기본값"));
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                self.cfg.transfer_dest_mode == TransferDestMode::CurrentFolder,
                                tr(lang, "원래 폴더 아래"),
                            )
                            .clicked()
                        {
                            self.cfg.transfer_dest_mode = TransferDestMode::CurrentFolder;
                            let _ = config::save(&self.cfg);
                        }
                        if ui
                            .selectable_label(
                                self.cfg.transfer_dest_mode == TransferDestMode::Fixed,
                                tr(lang, "지정된 폴더"),
                            )
                            .clicked()
                        {
                            self.cfg.transfer_dest_mode = TransferDestMode::Fixed;
                            if self.cfg.transfer_dest_folder.trim().is_empty() {
                                self.cfg.transfer_dest_folder =
                                    nfc_hangul(&config::pictures_dir().to_string_lossy());
                            }
                            let _ = config::save(&self.cfg);
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let rest = (ui.available_width() - 96.0).max(120.0);
                        let pictures = nfc_hangul(&config::pictures_dir().to_string_lossy());
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.cfg.transfer_dest_folder)
                                    .font(mono(12.0))
                                    .desired_width(rest)
                                    .hint_text(pictures),
                            )
                            .changed()
                        {
                            if !self.cfg.transfer_dest_folder.trim().is_empty() {
                                self.cfg.transfer_dest_mode = TransferDestMode::Fixed;
                            }
                            let _ = config::save(&self.cfg);
                        }
                        if toggle_btn(ui, tr(lang, "찾아보기…"), false).clicked() {
                            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                self.cfg.transfer_dest_folder = nfc_path_label(&d);
                                self.cfg.transfer_dest_mode = TransferDestMode::Fixed;
                                let _ = config::save(&self.cfg);
                            }
                        }
                    });
                    ui.label(egui::RichText::new(tr(lang, "원래 폴더 아래: 한 폴더로 보낼 때는 selected 하위폴더, 나누기면 pick/hold 등. 지정된 폴더는 위 경로입니다. 전송(Ctrl/⌘E)에서 바꿔도 다음은 다시 이 기본값입니다.")).font(mono(10.0)).color(INK_HELP));
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
                    ui.label(egui::RichText::new(tr(lang, "사진 배경")).font(prop(11.0)).color(INK_HEAD));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(tr(lang, "사진 표시 화면 배경색 — 프리셋 또는 HEX/RGB로 지정(Lightroom Develop 기본값은 50% 회색)")).font(mono(10.0)).color(INK_HELP));
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
                                            .color(if selected { theme::INK2 } else { INK_HELP }),
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
                        ui.painter().rect(sr, Rounding::same(4.0), Color32::from_rgb(cur[0], cur[1], cur[2]), Stroke::new(1.0_f32, theme::LINE3));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("HEX").font(mono(10.0)).color(INK_HEAD));
                        let resp = ui.add(egui::TextEdit::singleline(&mut self.bg_hex).font(mono(12.0)).desired_width(84.0).hint_text("#101010"));
                        if resp.changed() {
                            if let Some(rgb) = parse_hex_rgb(&self.bg_hex) {
                                self.cfg.photo_bg = Some(rgb);
                                let _ = config::save(&self.cfg);
                            }
                        }
                        // HEX 무효 입력 빨간 테두리(#69): 버퍼가 비지 않았는데 파싱 실패면 필드에 REJECT 테두리.
                        if !self.bg_hex.is_empty() && parse_hex_rgb(&self.bg_hex).is_none() {
                            ui.painter().rect_stroke(resp.rect, Rounding::same(2.0), Stroke::new(1.0_f32, theme::REJECT));
                        }
                        ui.add_space(8.0);
                        let mut rgb = self.photo_bg_rgb();
                        let mut changed = false;
                        ui.label(egui::RichText::new("R").font(mono(10.0)).color(INK_HEAD));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[0]).range(0..=255)).changed();
                        ui.label(egui::RichText::new("G").font(mono(10.0)).color(INK_HEAD));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[1]).range(0..=255)).changed();
                        ui.label(egui::RichText::new("B").font(mono(10.0)).color(INK_HEAD));
                        changed |= ui.add(egui::DragValue::new(&mut rgb[2]).range(0..=255)).changed();
                        if changed {
                            self.cfg.photo_bg = Some(rgb);
                            self.bg_hex = hex_str(rgb);
                            let _ = config::save(&self.cfg);
                        }
                    });

                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(tr(lang, "라벨")).font(prop(11.0)).color(INK_HEAD));
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
                    ui.label(egui::RichText::new(tr(lang, "단축키 재바인딩 UI는 v1.1 예정 — 현재 기본값 QWER 고정 표시")).font(mono(10.0)).color(INK_HELP));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr(lang, "별점 1~5 지정 · ` (백틱)으로 해제 — 라벨(QWER)과 독립으로 동시에 매겨집니다")).font(mono(10.0)).color(INK_HELP));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(tr(lang, "M = 촬영 위치 미니 지도(GPS 있는 사진) · A = AF 포인트 표시 — 토글 상태는 저장됩니다")).font(mono(10.0)).color(INK_HELP));

                    // ── COLOR TAGS (#27): 색별 커스텀 이름. 비우면 기본 색 이름 표시 ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new(tr(lang, "색 태그 이름")).font(prop(11.0)).color(INK_HEAD));
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(tr(lang, "색별 이름을 지정해 보정 방식 등 나만의 분류로 — ⇧1~5로 부여")).font(mono(10.0)).color(INK_HELP));
                    ui.add_space(4.0);
                    for (i, tag) in ColorTag::ALL.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let rgb = tag.color_rgb().unwrap_or([0x6b, 0x72, 0x80]);
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 6.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.cfg.tag_names[i])
                                        .hint_text(tag.default_name(lang))
                                        .desired_width(220.0),
                                )
                                .changed()
                            {
                                let _ = config::save(&self.cfg); // 태그 이름은 키 입력마다 즉시 저장(#69).
                            }
                        });
                    }

                    // ── CACHE (#22): 썸네일 디스크 캐시 사용량 + 비우기 ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new(tr(lang, "캐시")).font(prop(11.0)).color(INK_HEAD));
                    if self.cache_size.is_none() {
                        self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                    }
                    let size = self.cache_size.unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(trf(lang, "썸네일 캐시 사용량 · {}", &[&fmt_bytes(size)])).font(mono(11.0)).color(theme::INK2));
                        if toggle_btn(ui, tr(lang, "캐시 비우기"), false).clicked() {
                            let _ = cache::clear(&config::cache_dir());
                            self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                            self.toast_info(tr(lang, "썸네일 캐시를 비웠습니다").into());
                        }
                        if toggle_btn(ui, tr(lang, "새로고침"), false).clicked() {
                            self.cache_size = Some(cache::dir_size(&config::cache_dir()));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(tr(lang, "자동 상한")).font(mono(11.0)).color(theme::INK2));
                        if ui.add(egui::DragValue::new(&mut self.cfg.cache_limit_mb).speed(64.0).range(0..=1_048_576).suffix(" MB")).changed() {
                            let _ = config::save(&self.cfg); // 캐시 상한 변경 즉시 저장(#69).
                        }
                        ui.label(egui::RichText::new(tr(lang, "(0 = 무제한)")).font(mono(10.0)).color(INK_HELP));
                    });
                    ui.label(egui::RichText::new(tr(lang, "상한을 넘으면 오래된 썸네일부터 자동 삭제 — 폴더 열 때·설정 변경 시 정리됩니다.")).font(mono(10.0)).color(INK_HELP));
                    ui.label(egui::RichText::new(tr(lang, "폴더를 다시 열어도 재디코딩 없이 즉시 표시됩니다.")).font(mono(10.0)).color(INK_HELP));
                    // 캐시 경로: 클릭하면 OS 파일 관리자에서 캐시 폴더를 연다(#69). hover 시 밝게 + 손가락 커서.
                    let cache_path = config::cache_dir();
                    let cache_path_str = cache_path.to_string_lossy().to_string();
                    let cache_font = mono(9.5);
                    let galley = ui.painter().layout_no_wrap(cache_path_str.clone(), cache_font.clone(), INK_HELP);
                    let (cp_rect, cp_resp) = ui.allocate_exact_size(galley.size(), Sense::click());
                    let cp_col = if cp_resp.hovered() { theme::INK2 } else { INK_HELP };
                    ui.painter().text(cp_rect.left_top(), Align2::LEFT_TOP, &cache_path_str, cache_font, cp_col);
                    if cp_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if cp_resp.clicked() {
                        reveal_in_file_manager(&cache_path);
                    }

                    // ── RESET (#69): 모든 설정을 기본값으로 — 2단 인라인 확인(모달 없이) ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new(tr(lang, "초기화")).font(prop(11.0)).color(INK_HEAD));
                    if !self.settings_reset_armed {
                        if toggle_btn(ui, tr(lang, "기본값 복원"), false).clicked() {
                            self.settings_reset_armed = true;
                        }
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(tr(lang, "모든 설정을 기본값으로 되돌립니다")).font(mono(11.0)).color(theme::WARN));
                            // '복원'은 경고색(WARN) 테두리 버튼으로 위험 동작임을 알린다. 실행은 패널을 그린 뒤
                            // 한 곳(do_reset)에서 — 재동기화·폰트 교체에 ctx가 필요하고 self 대량 변경을 피하기 위해.
                            let restore = ui.add(
                                egui::Button::new(egui::RichText::new(tr(lang, "복원")).font(prop(12.0)).color(theme::WARN))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::new(1.0_f32, theme::WARN)),
                            );
                            if restore.clicked() {
                                do_reset = true;
                            }
                            if toggle_btn(ui, tr(lang, "취소"), false).clicked() {
                                self.settings_reset_armed = false;
                            }
                        });
                    }

                    // ── ABOUT / LINKS (#18): 버전·릴리즈·이슈·제작자·cosly ──
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new(tr(lang, "정보")).font(prop(11.0)).color(INK_HEAD));
                    ui.label(egui::RichText::new(format!("RawBlow v{}", env!("CARGO_PKG_VERSION"))).font(mono(11.0)).color(theme::INK2));
                    ui.add_space(6.0);
                    link_label(ui, tr(lang, "최신 버전 받기 · GitHub Releases"), "https://github.com/ascoeur9/rawblow/releases");
                    link_label(ui, tr(lang, "버그 제보 · GitHub Issues"), "https://github.com/ascoeur9/rawblow/issues");
                    // 오픈소스 라이센스 고지(#39): 포함된 구성요소 목록 + 전문 페이지.
                    ui.add_space(8.0);
                    if toggle_btn(ui, tr(lang, "오픈소스 라이센스"), false).clicked() {
                        self.licenses = Some(crate::licenses::LicensesPage::new());
                    }
                    ui.label(egui::RichText::new(tr(lang, "이 프로그램이 포함한 오픈소스 구성요소 목록과 라이센스 전문")).font(mono(10.0)).color(INK_HELP));
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(lang, "만든 사람 · 하레 (Hare)")).font(prop(11.5)).color(theme::INK2));
                    ui.horizontal(|ui| {
                        link_label(ui, "X · @ascoeur9", "https://x.com/ascoeur9");
                        ui.label(egui::RichText::new("·").font(mono(10.0)).color(INK_HELP));
                        link_label(ui, "X · @hare_kig", "https://x.com/hare_kig");
                    });
                    ui.add_space(10.0);
                    link_label(ui, tr(lang, "투네이션으로 후원하기"), "https://toon.at/donate/hare");
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(tr(lang, "마음에 드시나요? 그럼 cosly도 이용해보세요.")).font(prop(11.5)).color(theme::INK2));
                    link_label(ui, "https://cosly.link", "https://cosly.link");
                });
            });
        // Esc = 돌아가기(#69). 라이센스 페이지가 떠 있으면 그 페이지가 Esc를 처리한다(실제로 licenses가
        // Some이면 ui_settings가 호출되지 않지만, 방어적으로 가드). has_modal이 전역 키를 막으므로 여기가
        // 설정 화면에서 Esc의 유일한 소비처다.
        if self.licenses.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            go_back = true;
        }
        // 기본값 복원 실행(#69): 사용자 데이터(폴더 히스토리·다이얼로그 마지막 사용 옵션)는 보존하고
        // 나머지 설정만 Config::default()로. 보존 필드는 self.cfg를 덮어쓰기 前에 clone out한다.
        if do_reset {
            let last_folder = self.cfg.last_folder.clone();
            let recent_folders = self.cfg.recent_folders.clone();
            let transfer_defaults = self.cfg.transfer_defaults.clone();
            let organize_defaults = self.cfg.organize_defaults.clone();
            let prev_lang = self.lang;
            self.cfg = Config::default();
            self.cfg.last_folder = last_folder;
            self.cfg.recent_folders = recent_folders;
            self.cfg.transfer_defaults = transfer_defaults;
            self.cfg.organize_defaults = organize_defaults;
            // 라이브 미러 재동기화(설정 컨트롤이 하던 것과 동일하게).
            self.show_exif = self.cfg.show_exif;
            self.show_hist = self.cfg.show_histogram;
            self.show_map = self.cfg.show_map;
            self.show_af = self.cfg.show_af;
            self.grid_cols = self.cfg.grid_cols.clamp(4, 12);
            self.lang = crate::i18n::effective_lang(&self.cfg);
            if self.lang != prev_lang {
                // 언어가 바뀌면 폰트 primary도 교체(#32 후속: 세로 어긋남 방지).
                crate::fonts::install(ctx, self.lang);
            }
            self.bg_hex = hex_str(self.photo_bg_rgb());
            // 정렬도 기본값(촬영시간순)으로 즉시 반영: 다른 미러와 달리 정렬은 self.sort 재설정 +
            // 재정렬이 필요하다. set_sort_order로 self.sort·cfg.sort·화면 순서를 함께 맞춘다
            // (안 하면 설정 UI는 기본값을, 화면은 이전 정렬을 보여 다음 폴더 열기 전까지 어긋남).
            let target_sort = self.cfg.sort;
            self.set_sort_order(target_sort);
            self.settings_reset_armed = false;
            let _ = config::save(&self.cfg);
            self.schedule_cache_trim(); // 기본 상한으로 캐시 정리.
            self.toast_info(tr(self.lang, "설정을 기본값으로 되돌렸습니다").into());
        }
        if go_back {
            self.settings_back();
        }
        self.grid_cols = self.cfg.grid_cols.clamp(4, 12);
        self.show_exif = self.cfg.show_exif;
        self.show_hist = self.cfg.show_histogram;
    }

    /// 설정 화면 닫기(#69): '돌아가기' 버튼과 Esc가 공유하는 단일 동작 — 저장 + 변경된 상한으로
    /// 캐시 정리 후 닫는다. 한 곳에 모아 버튼과 Esc 동작이 어긋나지 않게 한다.
    fn settings_back(&mut self) {
        self.show_settings = false;
        let _ = config::save(&self.cfg);
        self.schedule_cache_trim(); // 변경된 상한으로 캐시 정리.
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
                    ui.label(egui::RichText::new(trf(lang, "{} 구성요소", &[&total.to_string()])).font(mono(10.5)).color(INK_HEAD));
                    if !generated.is_empty() {
                        ui.label(egui::RichText::new(format!("· {}", generated)).font(mono(10.5)).color(INK_HELP));
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
                        .color(INK_HELP),
                );
                ui.add_space(12.0);
                if let Some(c) = page.doc.crates.get(page.selected) {
                    ui.label(egui::RichText::new(format!("{} v{}", c.n, c.v)).font(prop(14.0)).color(theme::INK));
                    ui.label(egui::RichText::new(c.l.as_str()).font(mono(11.0)).color(INK_HEAD));
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
