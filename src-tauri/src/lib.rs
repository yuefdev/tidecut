// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(render::RenderManager::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            analysis_cancel::begin_smart_shorts_analysis,
            analysis_cancel::cancel_smart_shorts_analysis,
            analysis_cancel::finish_smart_shorts_analysis,
            auto_reframe::check_auto_reframe_runtime,
            auto_reframe::setup_auto_reframe_runtime,
            auto_reframe::analyze_auto_reframe,
            auto_reframe::detect_facecam,
            smart_shorts::check_fireworks_smart_shorts,
            smart_shorts::rank_smart_short_candidates,
            render::get_render_capabilities,
            render::probe_media_duration,
            render::analyze_audio_loudness,
            audio_energy::analyze_audio_energy,
            laughter_analysis::analyze_laughter,
            beat_analysis::analyze_beats,
            voice_rider::analyze_voice_rider,
            voice_rider::analyze_speech_activity,
            speech_analysis::analyze_speech_transcript,
            audio_cleanup::prepare_audio_cleanup,
            clearvoice_experimental::prepare_mossformer_test_cleanup,
            render::ensure_ffmpeg,
            render::start_render,
            render::start_audio_export,
            render::cancel_render,
            render::get_render_job,
            render::list_render_jobs,
            render::start_proxy,
            render::get_proxy_cache_stats,
            render::prune_proxy_cache,
            project::save_project,
            project::load_project,
            project::write_recovery_snapshot,
            project::list_recovery_snapshots,
            project::load_recovery_snapshot,
            project::discard_recovery_snapshot,
            project::inspect_media,
            project::find_missing_media,
            project::relink_media,
            captions::read_caption_file,
            captions::write_caption_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
#[cfg(all(test, target_os = "windows"))]
#[link(name = "resource", kind = "static")]
extern "C" {}

mod audio_cleanup;
mod audio_energy;
mod analysis_cancel;
mod auto_reframe;
mod beat_analysis;
mod captions;
mod clearvoice_experimental;
mod laughter_analysis;
mod project;
mod render;
mod smart_shorts;
mod speech_analysis;
mod voice_rider;
