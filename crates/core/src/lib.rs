//! Core domain models, presentation state machine, and command/event bus for Overture.

pub mod models;
pub mod state;

pub use models::*;
pub use state::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_song() -> Song {
        let mut song = Song::new_draft("Castelo Forte", "Martinho Lutero", "public_domain");
        song.id = 42;
        song.slides = vec![
            SongSlide {
                id: 1,
                song_id: 42,
                order: 0,
                text: "Castelo forte é nosso Deus,\nEspada e bom escudo".to_string(),
                forced_break: false,
                duration_secs: Some(2),
            },
            SongSlide {
                id: 2,
                song_id: 42,
                order: 1,
                text: "Com seu poder defende os seus\nEm todo transe agudo".to_string(),
                forced_break: false,
                duration_secs: Some(3),
            },
        ];
        song
    }

    #[test]
    fn hex_color_roundtrip() {
        let c = RgbaColor::from_hex("#1E293B").expect("valid 6-char hex");
        assert_eq!(c, RgbaColor::new(0x1E, 0x29, 0x3B, 0xFF));
        assert_eq!(c.to_hex(), "#1E293B");

        let ca = RgbaColor::from_hex("#000000CC").expect("valid 8-char hex");
        assert_eq!(ca, RgbaColor::new(0, 0, 0, 0xCC));
        assert_eq!(ca.to_hex(), "#000000CC");
    }

    #[test]
    fn state_engine_navigation_and_modes() {
        let engine = StateEngine::new();
        let mut events = engine.subscribe_events();

        let song = sample_song();
        let item = LivePresentationItem::from_song(&song, Some(10), ThemeConfig::default());
        let st = engine.dispatch(CoreCommand::PresentItem {
            item: Box::new(item),
            start_slide: 0,
        });
        assert_eq!(st.current_slide_index, 0);
        assert_eq!(st.total_slides(), 2);
        assert!(st.current_slide().unwrap().text.starts_with("Castelo forte"));
        assert!(st.next_slide().unwrap().text.starts_with("Com seu poder"));

        // Verify ItemExecuted event was emitted for execution_log tracking
        let first_event = events.try_recv().expect("expected ItemExecuted");
        match first_event {
            CoreEvent::ItemExecuted {
                service_item_id,
                song_id,
            } => {
                assert_eq!(service_item_id, Some(10));
                assert_eq!(song_id, Some(42));
            }
            other => panic!("unexpected first event: {other:?}"),
        }

        let st = engine.dispatch(CoreCommand::NextSlide);
        assert_eq!(st.current_slide_index, 1);
        assert!(st.next_slide().is_none());

        let st = engine.dispatch(CoreCommand::ToggleBlackScreen);
        assert_eq!(st.display_mode, DisplayMode::BlackScreen);
        let st = engine.dispatch(CoreCommand::ToggleBlackScreen);
        assert_eq!(st.display_mode, DisplayMode::Normal);

        let st = engine.dispatch(CoreCommand::ToggleLogo);
        assert_eq!(st.display_mode, DisplayMode::Logo);
    }

    #[test]
    fn countdown_scoreboard_and_auto_advance() {
        let engine = StateEngine::new();
        let song = sample_song();
        engine.dispatch(CoreCommand::PresentItem {
            item: Box::new(LivePresentationItem::from_song(
                &song,
                None,
                ThemeConfig::default(),
            )),
            start_slide: 0,
        });

        engine.dispatch(CoreCommand::StartCountdown {
            label: "Início do Culto".to_string(),
            seconds: 2,
            show_on_main: true,
        });
        engine.dispatch(CoreCommand::SetAutoPlay {
            enabled: true,
            default_duration_secs: Some(5),
        });
        engine.dispatch(CoreCommand::StartStageTimer);

        let st = engine.dispatch(CoreCommand::TickSecond);
        assert_eq!(st.countdown.remaining_secs, 1);
        assert_eq!(st.current_slide_index, 0); // slide 0 duration is 2s
        assert_eq!(st.stage_elapsed_secs, 1);

        let st = engine.dispatch(CoreCommand::TickSecond);
        assert_eq!(st.countdown.remaining_secs, 0);
        assert!(!st.countdown.active);
        assert_eq!(st.current_slide_index, 1); // auto-advanced after 2s!

        let st = engine.dispatch(CoreCommand::IncrementScore {
            team_a_delta: 3,
            team_b_delta: 1,
        });
        assert_eq!(st.scoreboard.team_a_score, 3);
        assert_eq!(st.scoreboard.team_b_score, 1);
    }
}
