use super::*;

fn faces() -> Vec<crate::fonts::FontFace> {
    let mut faces = vec![crate::fonts::FontFace {
        name: "ui-SourceSans3-Regular".into(),
        chain: "ui",
        bytes: Arc::new(
            include_bytes!("../../../editor/fonts/ui-SourceSans3-Regular.ttf").to_vec(),
        ),
        tweak: crate::fonts::FaceTweak::default(),
    }];
    faces.extend(crate::fonts::system_faces());
    faces
}

fn shape(text: &str, width: Option<f32>) -> (TextState, Shaped) {
    let mut state = TextState::new(&faces(), "en-US");
    let mut request = Request::new(text, 20.0);
    request.width = width;
    request.markup = true;
    let shaped = state.layout(&request.as_ref(), 1.0);
    (state, shaped)
}

/// A face the machine happens to have must not change a measurement, or
/// two platforms answer differently and a width in state desyncs a replay.
#[test]
fn a_system_face_does_not_reach_a_measurement() {
    let request = Request::new("measure me", 24.0);
    // The project's face alone, and the same with a system face behind it.
    let own = vec![faces()[0].clone()];
    let mut with_system = own.clone();
    with_system.push(crate::fonts::FontFace {
        name: "system:pretend".into(),
        chain: "system",
        bytes: std::sync::Arc::new(
            include_bytes!("../../../editor/fonts/mono-JetBrainsMono-Regular.ttf").to_vec(),
        ),
        tweak: crate::fonts::FaceTweak::default(),
    });
    let strict = TextState::new(&own, "en-US").measure(&request);
    let loose = TextState::new(&with_system, "en-US").measure(&request);
    assert_eq!(strict, loose, "a system face changed a measurement");
}

/// Two sizes a hair apart share a bucket, so the atlas holds one set of
/// glyphs for both and a zooming camera re-shapes rarely.
#[test]
fn near_sizes_land_in_one_bucket() {
    assert!((bucket(24.0) - bucket(24.2)).abs() < f32::EPSILON);
    assert!(bucket(24.0) >= 24.0, "a bucket never shrinks the text");
    assert!(bucket(48.0) > bucket(24.0));
}

/// The world reads the same pixels the widgets do, so the atlas has to
/// hold them rather than hand them straight to egui.
#[test]
fn a_shaped_glyph_lands_in_the_atlas_and_moves_its_revision() {
    let (state, shaped) = shape("A", None);
    assert_eq!(shaped.quads.len(), 1);
    let atlas = state.atlas();
    assert!(atlas.revision() > 0, "rasterising bumps the revision");
    let side = atlas.side();
    assert_eq!(atlas.rgba().len(), side * side * 4);
    // The quad's UV names the box the glyph was written into; something
    // in it has to be opaque, or the upload carries nothing.
    let uv = shaped.quads[0].uv;
    let x0 = (uv.min.x * side as f32) as usize;
    let y0 = (uv.min.y * side as f32) as usize;
    let x1 = (uv.max.x * side as f32).ceil() as usize;
    let y1 = (uv.max.y * side as f32).ceil() as usize;
    let inked =
        (y0..y1).any(|row| (x0..x1).any(|column| atlas.rgba()[(row * side + column) * 4 + 3] > 0));
    assert!(inked, "the glyph's box in the atlas is blank");
}

/// Filling the page doubles it rather than starting over, so a glyph
/// already rasterised keeps its pixels and is never drawn again.
#[test]
fn an_atlas_that_fills_up_doubles_instead_of_starting_over() {
    let mut state = TextState::new(&faces(), "en-US");
    let ask = |state: &mut TextState, text: String| state.shape(&Request::new(&text, 64.0));
    let opened = state.atlas().side();
    // Enough distinct glyphs at a size that fills a small page.
    for c in 'a'..='z' {
        ask(&mut state, c.to_string());
    }
    for c in 'A'..='Z' {
        ask(&mut state, c.to_string());
    }
    let grown = state.atlas().side();
    assert!(
        grown > opened,
        "the atlas stayed at {opened} and wiped instead"
    );
    assert_eq!(state.atlas().rgba().len(), grown * grown * 4);
    // The first glyph still has ink where its UV says, which a reset
    // would have taken away.
    let shaped = ask(&mut state, "a".to_owned());
    let uv = shaped.quads[0].uv;
    let rgba = state.atlas().rgba();
    let x0 = (uv.min.x * grown as f32) as usize;
    let y0 = (uv.min.y * grown as f32) as usize;
    let x1 = (uv.max.x * grown as f32).ceil() as usize;
    let y1 = (uv.max.y * grown as f32).ceil() as usize;
    let inked = (y0..y1).any(|row| (x0..x1).any(|column| rgba[(row * grown + column) * 4 + 3] > 0));
    assert!(inked, "the glyph's box in the grown atlas is blank");
}

/// A second consumer must see a stable atlas: shaping the same run twice
/// comes from the cache and writes nothing new.
#[test]
fn shaping_the_same_run_twice_writes_the_atlas_once() {
    let mut state = TextState::new(&faces(), "en-US");
    let request = Request::new("steady", 20.0);
    state.shape(&request);
    let after_first = state.atlas().revision();
    state.shape(&request);
    assert_eq!(after_first, state.atlas().revision());
}

#[test]
fn latin_text_shapes_to_one_quad_per_letter_left_to_right() {
    let (_, shaped) = shape("abc", None);
    assert_eq!(shaped.quads.len(), 3);
    assert!(shaped.quads[0].rect.min.x < shaped.quads[1].rect.min.x);
    assert!(shaped.quads[1].rect.min.x < shaped.quads[2].rect.min.x);
    assert!(shaped.size.x > 0.0 && shaped.size.y > 0.0);
}

#[test]
fn a_marked_up_colour_lands_on_its_own_glyphs_only() {
    let (_, shaped) = shape("a[color=#ff0000]b[/color]c", None);
    let colours: Vec<Option<Color32>> = shaped.quads.iter().map(|q| q.color).collect();
    assert_eq!(colours, [None, Some(Color32::from_rgb(255, 0, 0)), None]);
}

#[test]
fn a_width_breaks_a_long_line_into_more_than_one() {
    let (_, one) = shape("one two three four five six", None);
    let (_, wrapped) = shape("one two three four five six", Some(80.0));
    assert!(wrapped.size.y > one.size.y, "wrapping adds rows");
    assert!(wrapped.size.x <= 80.0 + f32::EPSILON);
}

#[test]
fn hebrew_runs_right_to_left_when_a_face_covers_it() {
    let (mut state, shaped) = shape("שלום", None);
    if !state.covers('ש') {
        eprintln!("skipped: no Hebrew face on this machine");
        return;
    }
    assert_eq!(shaped.quads.len(), 4);
    // The first letter of the word is drawn at the right edge.
    assert!(shaped.quads[0].rect.min.x > shaped.quads[3].rect.min.x);
}

#[test]
fn arabic_letters_join_into_contextual_forms_when_a_face_covers_it() {
    let (mut state, joined) = shape("سلام", None);
    if !state.covers('س') {
        eprintln!("skipped: no Arabic face on this machine");
        return;
    }
    let (_, isolated) = shape("س ل ا م", None);
    // Joined forms are narrower than the same letters set apart.
    assert!(joined.size.x < isolated.size.x);
    assert_eq!(isolated.quads.len(), 4);
    // Lam and alef fuse into one glyph, which only a shaper produces —
    // and only on a face that carries the ligature, which the one Windows
    // picks does not. Ask this face before asserting it.
    let (_, ligature) = shape("\u{644}\u{627}", None);
    if ligature.quads.len() == 1 {
        assert_eq!(joined.quads.len(), 3);
    }
}

fn tweaked(tweak: crate::fonts::FaceTweak) -> Vec<crate::fonts::FontFace> {
    let mut faces = vec![faces()[0].clone()];
    faces[0].tweak = tweak;
    faces
}

fn plain(text: &str, size: f32) -> Request {
    Request::new(text, size)
}

/// The import's `scale` sizes the face the way egui's `FontTweak` does:
/// its glyphs, their advances and, for the chain's first face, the line.
#[test]
fn a_face_scale_draws_and_measures_its_glyphs_larger() {
    let request = plain("abc", 20.0);
    let mut plain_state = TextState::new(&tweaked(crate::fonts::FaceTweak::default()), "en-US");
    let mut doubled = TextState::new(
        &tweaked(crate::fonts::FaceTweak {
            scale: 2.0,
            ..Default::default()
        }),
        "en-US",
    );
    let (one, two) = (plain_state.measure(&request), doubled.measure(&request));
    assert!((two.x - one.x * 2.0).abs() < 1.0, "{one:?} against {two:?}");
    assert!((two.y - one.y * 2.0).abs() < 1.0, "{one:?} against {two:?}");
    let (small, large) = (plain_state.shape(&request), doubled.shape(&request));
    let tall = |shaped: &Shaped| shaped.quads[0].rect.height();
    assert!(
        (tall(&large) - tall(&small) * 2.0).abs() < 2.0,
        "{} against {}",
        tall(&small),
        tall(&large)
    );
    assert_eq!(large.size, two, "drawing and measuring disagree");
}

/// A fallback face's `scale` reaches the glyphs it draws and no others.
#[test]
fn a_fallback_face_scales_only_its_own_glyphs() {
    let icon = |scale: f32| crate::fonts::FontFace {
        name: "icon-Phosphor-Fill".into(),
        chain: "icon",
        bytes: Arc::new(include_bytes!("../../../editor/fonts/icon-Phosphor-Fill.ttf").to_vec()),
        tweak: crate::fonts::FaceTweak {
            scale,
            ..Default::default()
        },
    };
    let request = plain("a\u{e3ee}", 20.0);
    let mut quads = Vec::new();
    let mut sizes = Vec::new();
    for scale in [1.0, 2.0] {
        let faces = vec![faces()[0].clone(), icon(scale)];
        let mut state = TextState::new(&faces, "en-US");
        let shaped = state.shape(&request);
        assert_eq!(shaped.quads.len(), 2, "the letter and the icon both drew");
        quads.push((shaped.quads[0].rect, shaped.quads[1].rect));
        sizes.push((state.measure(&request), shaped.size));
    }
    let ((letter, mark), (letter_scaled, mark_scaled)) = (quads[0], quads[1]);
    assert_eq!(
        letter.size(),
        letter_scaled.size(),
        "the letter's face did not scale"
    );
    assert!(
        (mark_scaled.height() - mark.height() * 2.0).abs() < 2.0,
        "{mark:?} against {mark_scaled:?}"
    );
    assert!(
        sizes[1].0.x > sizes[0].0.x + 10.0,
        "the measure kept the icon's width"
    );
    assert!(
        (sizes[1].0.y - sizes[0].0.y).abs() < 0.5,
        "a fallback moved the line"
    );
    assert_eq!(sizes[1].0, sizes[1].1, "drawing and measuring disagree");
}

/// `y_offset` lowers the face's glyphs by a fraction of its size and
/// leaves the box where it was.
#[test]
fn a_face_y_offset_lowers_its_glyphs_and_keeps_the_layout() {
    let request = plain("abc", 20.0);
    let mut level = TextState::new(&tweaked(crate::fonts::FaceTweak::default()), "en-US");
    let mut lowered = TextState::new(
        &tweaked(crate::fonts::FaceTweak {
            y_offset: 0.5,
            ..Default::default()
        }),
        "en-US",
    );
    let (high, low) = (level.shape(&request), lowered.shape(&request));
    assert_eq!(high.size, low.size);
    let drop = low.quads[0].rect.min.y - high.quads[0].rect.min.y;
    assert!((drop - 10.0).abs() <= 1.0, "dropped {drop} for half of 20");
    assert_eq!(level.measure(&request), lowered.measure(&request));
}

/// `hinting = false` rasterises the face's glyphs with swash's hinter off.
#[test]
fn a_face_with_hinting_off_rasterises_unhinted() {
    let hinted = |hinting: Option<bool>| {
        let mut state = TextState::new(
            &tweaked(crate::fonts::FaceTweak {
                hinting,
                ..Default::default()
            }),
            "en-US",
        );
        state.shape(&plain("ab", 20.0));
        let flags: Vec<bool> = state
            .atlas
            .keys()
            .map(|key| !key.flags.contains(CacheKeyFlags::DISABLE_HINTING))
            .collect();
        assert!(!flags.is_empty(), "nothing was rasterised");
        flags
    };
    assert!(hinted(None).iter().all(|on| *on));
    assert!(hinted(Some(true)).iter().all(|on| *on));
    assert!(hinted(Some(false)).iter().all(|on| !*on));
}

#[test]
fn a_picture_reserves_its_box_on_the_line() {
    let (_, shaped) = shape("x[img=icon.png width=40]y", None);
    assert_eq!(shaped.pictures.len(), 1);
    let picture = &shaped.pictures[0];
    assert!((picture.rect.width() - 40.0).abs() < f32::EPSILON);
    let after = shaped
        .quads
        .iter()
        .map(|q| q.rect.min.x)
        .fold(0.0, f32::max);
    assert!(
        after >= picture.rect.min.x + 30.0,
        "the next glyph clears the picture"
    );
}

fn bundled(name: &str, chain: &'static str, bytes: &'static [u8]) -> crate::fonts::FontFace {
    crate::fonts::FontFace {
        name: name.into(),
        chain,
        bytes: Arc::new(bytes.to_vec()),
        tweak: crate::fonts::FaceTweak::default(),
    }
}

/// The editor's own three text faces, with no system face behind them.
fn editor_faces() -> Vec<crate::fonts::FontFace> {
    vec![
        bundled(
            "ui-SourceSans3-Regular",
            "ui",
            include_bytes!("../../../editor/fonts/ui-SourceSans3-Regular.ttf"),
        ),
        bundled(
            "heading-SourceSans3-Semibold",
            "heading",
            include_bytes!("../../../editor/fonts/heading-SourceSans3-Semibold.ttf"),
        ),
        bundled(
            "mono-JetBrainsMono-Regular",
            "mono",
            include_bytes!("../../../editor/fonts/mono-JetBrainsMono-Regular.ttf"),
        ),
    ]
}

/// `text` at 20 pixels over the editor's faces, as `edit` changes it.
fn shaped_with(text: &str, edit: impl FnOnce(&mut Request)) -> (TextState, Rc<Shaped>) {
    let mut state = TextState::new(&editor_faces(), "en-US");
    let mut request = Request::new(text, 20.0);
    edit(&mut request);
    let shaped = state.shape(&request);
    (state, shaped)
}

#[test]
fn an_underline_draws_one_bar_under_the_glyphs_and_double_draws_two() {
    let (_, plain) = shaped_with("under", |_| {});
    assert!(plain.lines.is_empty());
    let (_, single) = shaped_with("under", |r| {
        r.options.decoration.underline = Underline::Single;
    });
    assert_eq!(single.lines.len(), 1);
    let bar = single.lines[0].rect;
    let glyphs = single
        .quads
        .iter()
        .fold(Rect::NOTHING, |all, quad| all.union(quad.rect));
    assert!(bar.min.y > glyphs.center().y, "the bar sits low: {bar:?}");
    assert!(
        bar.width() >= glyphs.width() * 0.9,
        "it runs under the word"
    );
    let (_, double) = shaped_with("under", |r| {
        r.options.decoration.underline = Underline::Double;
    });
    assert_eq!(double.lines.len(), 2);
    assert!(double.lines[1].rect.min.y > double.lines[0].rect.max.y);
}

#[test]
fn strikethrough_crosses_the_middle_and_overline_sits_on_top() {
    let (_, shaped) = shaped_with("xx", |r| {
        r.options.decoration.strikethrough = true;
        r.options.decoration.overline = true;
        r.options.decoration.overline_color = Some(Color32::RED);
    });
    assert_eq!(shaped.lines.len(), 2);
    let (strike, over) = (shaped.lines[0], shaped.lines[1]);
    assert!(
        over.rect.max.y < strike.rect.min.y,
        "{over:?} above {strike:?}",
        over = over.rect,
        strike = strike.rect
    );
    assert_eq!(over.color, Some(Color32::RED));
    assert_eq!(strike.color, None, "an unnamed colour is the label's");
}

#[test]
fn a_marked_underline_runs_under_its_own_words_only() {
    let (_, shaped) = shaped_with("plain [u]marked[/u] plain", |r| r.markup = true);
    assert_eq!(shaped.lines.len(), 1);
    let bar = shaped.lines[0].rect;
    assert!(
        bar.min.x > 20.0 && bar.width() < shaped.size.x / 2.0,
        "{bar:?}"
    );
}

#[test]
fn a_marked_colour_tints_the_line_under_it() {
    let (_, shaped) = shaped_with("[u][color=#00ff00]green[/color][/u]", |r| r.markup = true);
    assert_eq!(shaped.lines[0].color, Some(Color32::from_rgb(0, 255, 0)));
}

#[test]
fn truncating_cuts_the_line_and_ends_it_with_an_ellipsis() {
    let text = "a sentence far too long for its box";
    let (_, whole) = shaped_with(text, |_| {});
    let (_, cut) = shaped_with(text, |r| {
        r.width = Some(80.0);
        r.truncate = true;
    });
    assert!(!whole.elided);
    assert!(cut.elided, "nothing said it was cut");
    assert!(cut.quads.len() < whole.quads.len());
    let right = cut.quads.iter().map(|q| q.rect.max.x).fold(0.0, f32::max);
    assert!(right <= 80.5, "the cut line runs to {right}");
    assert!(cut.size.y < whole.size.y * 1.5, "it stayed on one line");
}

#[test]
fn truncating_at_the_start_keeps_the_end_of_the_text() {
    let text = "first middle last";
    let end = |at: TruncateAt| {
        shaped_with(text, |r| {
            r.width = Some(60.0);
            r.truncate = true;
            r.options.truncate_at = at;
        })
        .1
    };
    let kept = |shaped: &Shaped| -> Vec<u32> { shaped.quads.iter().map(|q| q.start).collect() };
    let (from_end, from_start) = (end(TruncateAt::End), end(TruncateAt::Start));
    assert!(
        kept(&from_end).contains(&0),
        "an end cut keeps the first letter"
    );
    let last = u32::try_from(text.len() - 1).unwrap();
    assert!(
        kept(&from_start).contains(&last),
        "a start cut keeps the last letter"
    );
    assert!(!kept(&from_start).contains(&0));
    assert!(end(TruncateAt::Middle).elided);
}

#[test]
fn max_lines_wraps_that_far_and_truncate_ends_the_last_with_an_ellipsis() {
    let text = "one two three four five six seven eight nine ten";
    let (_, all) = shaped_with(text, |r| r.width = Some(60.0));
    let (_, two) = shaped_with(text, |r| {
        r.width = Some(60.0);
        r.options.max_lines = 2;
    });
    let (_, cut) = shaped_with(text, |r| {
        r.width = Some(60.0);
        r.truncate = true;
        r.options.max_lines = 2;
    });
    assert!(two.size.y < all.size.y, "the lines past two were kept");
    assert!(!two.elided);
    assert!((cut.size.y - two.size.y).abs() < 1.0);
    assert!(cut.elided);
}

#[test]
fn max_height_drops_the_lines_that_start_below_it() {
    let text = "one two three four five six seven eight nine ten";
    let (_, all) = shaped_with(text, |r| r.width = Some(60.0));
    let (_, short) = shaped_with(text, |r| {
        r.width = Some(60.0);
        r.options.max_height = Some(30.0);
    });
    assert!(short.size.y < all.size.y);
    assert!(short.size.y <= 30.0 + 25.0, "one line past the top at most");
}

#[test]
fn justify_stretches_a_wrapped_line_to_the_width() {
    let text = "one two three four five six seven eight";
    let widest = |align: Align| {
        let (_, shaped) = shaped_with(text, |r| {
            r.width = Some(120.0);
            r.align = align;
        });
        // The first line's rightmost glyph.
        let top = shaped.quads[0].rect.min.y;
        shaped
            .quads
            .iter()
            .filter(|q| (q.rect.min.y - top).abs() < 8.0)
            .map(|q| q.rect.max.x)
            .fold(0.0, f32::max)
    };
    assert!(widest(Align::Justify) > widest(Align::Left) + 2.0);
    assert!(widest(Align::Justify) <= 120.5);
}

#[test]
fn right_sits_on_the_right_for_left_to_right_text_and_left_on_the_left() {
    let edge = |align: Align| {
        let (_, shaped) = shaped_with("hi", |r| {
            r.width = Some(200.0);
            r.align = align;
        });
        shaped.quads[0].rect.min.x
    };
    assert!(edge(Align::Right) > 150.0);
    assert!(edge(Align::Left) < 10.0);
    assert!((edge(Align::End) - edge(Align::Right)).abs() < 0.5);
}

#[test]
fn oblique_slants_the_upright_face_when_no_italic_ships() {
    let (state, _) = shaped_with("o", |r| r.slant = Slant::Oblique);
    assert!(
        state
            .atlas
            .keys()
            .all(|key| key.flags.contains(CacheKeyFlags::FAKE_ITALIC))
    );
}

#[test]
fn a_feature_changes_the_glyphs_it_names() {
    let (_, ligature) = shaped_with("office", |_| {});
    let (_, apart) = shaped_with("office", |r| {
        r.options.features = vec![Feature::parse("liga=0").unwrap()];
    });
    assert!(
        apart.quads.len() > ligature.quads.len(),
        "liga=0 kept the ligature: {} against {}",
        apart.quads.len(),
        ligature.quads.len()
    );
}

#[test]
fn a_font_name_shapes_in_that_family_ahead_of_the_chain() {
    let narrow = |name: &str| {
        let (_, shaped) = shaped_with("iiii", |r| r.options.font_name = name.to_string());
        shaped.size.x
    };
    let (_, wide) = shaped_with("MMMM", |r| r.options.font_name = "JetBrains Mono".into());
    assert!(
        (narrow("JetBrains Mono") - wide.size.x).abs() < 1.0,
        "a monospace face draws i as wide as M"
    );
    assert!(narrow("") < wide.size.x * 0.6);
}

#[test]
fn line_break_by_glyph_fills_the_line_where_by_word_leaves_a_gap() {
    let rows = |mode: LineBreak| {
        let (_, shaped) = shaped_with("aaa bbbbbbbbbb", |r| {
            r.width = Some(70.0);
            r.options.line_break = mode;
        });
        shaped.quads.iter().filter(|q| q.rect.min.y < 10.0).count()
    };
    assert!(rows(LineBreak::Glyph) > rows(LineBreak::Word));
}

#[test]
fn snapping_advances_puts_every_glyph_on_a_whole_pixel() {
    let (_, shaped) = shaped_with("abcdefgh", |r| {
        r.options.snap_advances = true;
        r.letter_spacing = 0.37;
    });
    let mut pens: Vec<f32> = shaped.quads.iter().map(|q| q.rect.min.x).collect();
    pens.dedup();
    assert!(pens.len() > 4);
}

#[test]
fn a_node_s_hinting_overrides_the_face_and_pixel_snap_marks_the_raster() {
    let flags = |hinting: Hinting, pixel_snap: bool| {
        let (state, _) = shaped_with("ab", |r| {
            r.options.hinting = hinting;
            r.options.pixel_snap = pixel_snap;
        });
        state.atlas.keys().map(|key| key.flags).collect::<Vec<_>>()
    };
    assert!(
        flags(Hinting::Off, false)
            .iter()
            .all(|f| f.contains(CacheKeyFlags::DISABLE_HINTING))
    );
    assert!(
        flags(Hinting::Auto, false)
            .iter()
            .all(|f| !f.contains(CacheKeyFlags::DISABLE_HINTING))
    );
    assert!(
        flags(Hinting::Auto, true)
            .iter()
            .all(|f| f.contains(CacheKeyFlags::PIXEL_FONT))
    );
}

#[test]
fn monospace_width_resizes_a_monospace_face_to_the_advance_asked() {
    let (_, plain) = shaped_with("mmmm", |r| r.family = "mono".into());
    let (_, wider) = shaped_with("mmmm", |r| {
        r.family = "mono".into();
        r.options.monospace_width = Some(24.0);
    });
    let last = |shaped: &Shaped| shaped.quads[3].rect.min.x;
    assert!(
        last(&wider) > last(&plain) + 10.0,
        "{} against {}",
        last(&wider),
        last(&plain)
    );
}

#[test]
fn a_wider_tab_pushes_the_text_after_it_further() {
    let after = |tab: u16| {
        let (_, shaped) = shaped_with("a\tb", |r| r.options.tab_width = tab);
        shaped.quads.last().unwrap().rect.min.x
    };
    assert!(after(8) > after(2) + 10.0);
}

#[test]
fn simple_shaping_lays_latin_out_as_complex_does() {
    let (_, complex) = shaped_with("plain text", |_| {});
    let (_, simple) = shaped_with("plain text", |r| r.options.shaping = Shaping::Simple);
    assert_eq!(complex.quads.len(), simple.quads.len());
    assert!((complex.size.x - simple.size.x).abs() < 2.0);
}

#[test]
fn a_marked_size_draws_its_run_larger() {
    let (_, shaped) = shaped_with("a[size=40]a[/size]", |r| r.markup = true);
    assert_eq!(shaped.quads.len(), 2);
    let (small, big) = (shaped.quads[0].rect, shaped.quads[1].rect);
    assert!(
        big.height() > small.height() * 1.6,
        "{small:?} against {big:?}"
    );
}

#[test]
fn a_marked_font_shapes_its_run_in_that_chain() {
    let (_, shaped) = shaped_with("ii[font=mono]ii[/font]", |r| r.markup = true);
    let width = |q: &[Quad]| q[1].rect.min.x - q[0].rect.min.x;
    let (ui, mono) = (width(&shaped.quads[0..2]), width(&shaped.quads[2..4]));
    assert!(mono > ui * 1.5, "mono {mono} against ui {ui}");
}

/// egui draws `heading` in the face its chain opens with; the shaper has to
/// pick the same one when nothing asks for another weight.
#[test]
fn a_heading_at_the_regular_weight_draws_in_the_chain_s_own_face() {
    let width = |family: &str, weight: u16| {
        let (_, shaped) = shaped_with("Heading", |r| {
            r.family = family.into();
            r.weight = weight;
        });
        shaped.size.x
    };
    let (regular, semibold) = (width("ui", 400), width("ui", 600));
    assert!(semibold > regular, "the two faces measure alike");
    assert!((width("heading", 400) - semibold).abs() < 0.5);
}
