//! Microfone sem o aviso nativo do WebView2 ("tauri.localhost quer usar seus microfones"): o app
//! libera só para as páginas dele, e quem pergunta à pessoa é o modal do Composer, uma vez.
//! No Linux o WebKitGTK nega o microfone se ninguém responder ao `permission-request`: o app
//! responde, liberando só áudio (nada de câmera ou tela) e só para as páginas dele.

#[cfg(windows)]
pub fn liberar<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    };
    use webview2_com::PermissionRequestedEventHandler;

    let _ = webview.with_webview(|w| unsafe {
        let Ok(core) = w.controller().CoreWebView2() else { return };
        let handler = PermissionRequestedEventHandler::create(Box::new(|_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut tipo = COREWEBVIEW2_PERMISSION_KIND::default();
            args.PermissionKind(&mut tipo)?;
            let mut uri = windows_core::PWSTR::null();
            args.Uri(&mut uri)?;
            let do_app = tauri::Url::parse(&webview2_com::take_pwstr(uri)).is_ok_and(|url| crate::eh_do_app(&url));
            if tipo == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE && do_app {
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
            }
            Ok(())
        }));
        let mut token = Default::default();
        let _ = core.add_PermissionRequested(&handler, &mut token);
    });
}

#[cfg(target_os = "linux")]
pub fn liberar<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    use webkit2gtk::glib::Cast;
    use webkit2gtk::{
        PermissionRequestExt, SettingsExt, UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebViewExt,
    };

    let _ = webview.with_webview(|w| {
        let view = w.inner();
        if let Some(settings) = WebViewExt::settings(&view) {
            settings.set_enable_media_stream(true);
            settings.set_enable_mediasource(true);
            settings.set_enable_webrtc(true);
        }
        view.connect_permission_request(|view, pedido| {
            // Outros pedidos (notificação, localização…) seguem o padrão do WebKit, que nega.
            let Some(midia) = pedido.downcast_ref::<UserMediaPermissionRequest>() else { return false };
            let do_app = view
                .uri()
                .and_then(|uri| tauri::Url::parse(uri.as_str()).ok())
                .is_some_and(|url| crate::eh_do_app(&url));
            if do_app && so_microfone(midia.is_for_audio_device(), midia.is_for_video_device()) {
                pedido.allow();
            } else {
                pedido.deny();
            }
            true
        });
    });
}

/// Libera só o microfone: pedido com câmera (ou compartilhamento de tela, que chega como vídeo) é negado.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn so_microfone(audio: bool, video: bool) -> bool {
    audio && !video
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn liberar<R: tauri::Runtime>(_: &tauri::Webview<R>) {}

#[cfg(test)]
mod tests {
    use super::so_microfone;

    #[test]
    fn libera_so_pedido_de_audio_sem_video() {
        assert!(so_microfone(true, false));
        assert!(!so_microfone(true, true));
        assert!(!so_microfone(false, true));
        assert!(!so_microfone(false, false));
    }
}
