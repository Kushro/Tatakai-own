//! System voice through an external process (`novel.tts.system`; contract section 0.12): no TTS crate, nothing
//! native linked. Linux: `espeak-ng -w <file>` (a WAV the reader plays and times) or, without it, `spd-say -w`
//! (speech-dispatcher speaks on its own; `Spoken::Native`). Windows: PowerShell `System.Speech` writing a WAV.
//! macOS: `say -o <file> --data-format=LEI16@22050`.

use std::path::Path;
use std::process::Stdio;

use tk_ui::backend::{TtsVoiceVm, UiResult};
use tk_ui::model::{ErrorKind, UiError};
use tokio::io::AsyncWriteExt;

use super::{err, which};

/// What [`speak`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spoken {
    /// The clip was written to the target file.
    File,
    /// The OS voice spoke on its own (already finished, or stopped).
    Native,
}

fn no_voice() -> UiError {
    UiError::new(
        ErrorKind::Other,
        "no system voice found: install espeak-ng or speech-dispatcher (spd-say)",
    )
}

/// espeak-ng words per minute for a rate percent (175 = 100 %).
pub fn espeak_wpm(rate: u8) -> u32 {
    (175 * u32::from(rate.clamp(50, 200))) / 100
}

/// espeak-ng pitch `0..=99` for a pitch percent (50 = 100 %).
pub fn espeak_pitch(pitch: u8) -> u32 {
    ((50 * u32::from(pitch.clamp(50, 200))) / 100).min(99)
}

/// speech-dispatcher / SAPI style `-100..=100` offset of a percent.
pub fn offset_of(percent: u8) -> i32 {
    (i32::from(percent.clamp(50, 200)) - 100).clamp(-100, 100)
}

/// `espeak-ng --voices` -> voices (`Pty Language Age/Gender VoiceName File Other`).
pub fn parse_espeak_voices(out: &str) -> Vec<TtsVoiceVm> {
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            if cols.len() < 4 {
                return None;
            }
            let lang = cols[1].to_owned();
            let gender = cols[2]
                .rsplit('/')
                .next()
                .filter(|g| *g == "M" || *g == "F")
                .map(str::to_owned);
            Some(TtsVoiceVm {
                id: lang.clone(),
                name: format!("{} ({lang})", cols[3].replace('_', " ")),
                lang: Some(lang),
                gender,
            })
        })
        .collect()
}

/// `spd-say -L` -> voices (`NAME LANGUAGE VARIANT`, first line is the header).
pub fn parse_spd_voices(out: &str) -> Vec<TtsVoiceVm> {
    out.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("NAME"))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            let name = cols.first()?.to_string();
            let lang = cols.get(1).map(|s| s.to_string());
            Some(TtsVoiceVm {
                id: name.clone(),
                name,
                lang,
                gender: None,
            })
        })
        .collect()
}

/// `say -v '?'` -> voices (`Name   en_US    # sample`; names may contain spaces).
pub fn parse_say_voices(out: &str) -> Vec<TtsVoiceVm> {
    out.lines()
        .filter_map(|l| {
            let left = l.split('#').next()?.trim_end();
            let (name, lang) = left.rsplit_once(char::is_whitespace)?;
            let name = name.trim();
            (!name.is_empty()).then(|| TtsVoiceVm {
                id: name.to_owned(),
                name: format!("{name} ({lang})"),
                lang: Some(lang.replace('_', "-")),
                gender: None,
            })
        })
        .collect()
}

/// `name|culture|gender` lines of the PowerShell voice listing.
pub fn parse_sapi_voices(out: &str) -> Vec<TtsVoiceVm> {
    out.lines()
        .filter_map(|l| {
            let mut p = l.trim().split('|');
            let name = p.next()?.trim().to_owned();
            if name.is_empty() {
                return None;
            }
            let lang = p.next().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
            let gender = p
                .next()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty() && s != "NotSet");
            Some(TtsVoiceVm {
                id: name.clone(),
                name,
                lang,
                gender,
            })
        })
        .collect()
}

const PS_VOICES: &str = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
$s.GetInstalledVoices() | Where-Object { $_.Enabled } | ForEach-Object { $_.VoiceInfo.Name + '|' + $_.VoiceInfo.Culture.Name + '|' + $_.VoiceInfo.Gender }";

const PS_SPEAK: &str = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
if ($env:TK_TTS_VOICE) { try { $s.SelectVoice($env:TK_TTS_VOICE) } catch {} }; $s.Rate = [int]$env:TK_TTS_RATE; \
$s.SetOutputToWaveFile($env:TK_TTS_OUT); $s.Speak([Console]::In.ReadToEnd()); $s.Dispose()";

async fn output(program: &Path, args: &[&str]) -> Option<String> {
    let program = program.to_path_buf();
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let out = super::quiet_command(&program)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })
    .await
    .ok()
    .flatten()
}

pub async fn voices() -> UiResult<Vec<TtsVoiceVm>> {
    if cfg!(windows) {
        let ps = which("powershell").ok_or_else(no_voice)?;
        let out = output(&ps, &["-NoProfile", "-NonInteractive", "-Command", PS_VOICES])
            .await
            .unwrap_or_default();
        return Ok(parse_sapi_voices(&out));
    }
    if cfg!(target_os = "macos") {
        let say = which("say").ok_or_else(no_voice)?;
        return Ok(parse_say_voices(
            &output(&say, &["-v", "?"]).await.unwrap_or_default(),
        ));
    }
    if let Some(espeak) = which("espeak-ng") {
        return Ok(parse_espeak_voices(
            &output(&espeak, &["--voices"]).await.unwrap_or_default(),
        ));
    }
    if let Some(spd) = which("spd-say") {
        return Ok(parse_spd_voices(&output(&spd, &["-L"]).await.unwrap_or_default()));
    }
    Err(no_voice())
}

/// Run `cmd` with `text` on stdin; `stop` kills it.
async fn run_with_stdin(
    mut cmd: tokio::process::Command,
    text: &str,
    stop: &tokio::sync::Notify,
) -> UiResult<bool> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(err)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).await.map_err(err)?;
        drop(stdin);
    }
    let stopped = stop.notified();
    tokio::pin!(stopped);
    tokio::select! {
        status = child.wait() => {
            let status = status.map_err(err)?;
            if !status.success() {
                let mut detail = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = e.read_to_string(&mut detail).await;
                }
                return Err(err(format!("the system voice failed ({status}): {}", detail.trim())));
            }
            Ok(true)
        }
        _ = &mut stopped => {
            let _ = child.kill().await;
            Ok(false)
        }
    }
}

/// Speak `text` with the OS voice into `target` (WAV) when the platform tool can write files, else natively.
pub async fn speak(
    text: &str,
    voice: Option<&str>,
    rate: u8,
    pitch: u8,
    target: &Path,
    stop: &tokio::sync::Notify,
) -> UiResult<Spoken> {
    let tmp = target.with_extension("part.wav");
    if cfg!(windows) {
        let ps = which("powershell").ok_or_else(no_voice)?;
        let mut cmd = tokio::process::Command::new(ps);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", PS_SPEAK])
            .env("TK_TTS_VOICE", voice.unwrap_or(""))
            .env("TK_TTS_RATE", (offset_of(rate) / 10).to_string())
            .env("TK_TTS_OUT", &tmp);
        if !run_with_stdin(cmd, text, stop).await? {
            return Ok(Spoken::Native);
        }
    } else if cfg!(target_os = "macos") {
        let say = which("say").ok_or_else(no_voice)?;
        let mut cmd = tokio::process::Command::new(say);
        cmd.arg("-o")
            .arg(&tmp)
            .arg("--data-format=LEI16@22050")
            .arg("-r")
            .arg(espeak_wpm(rate).to_string());
        if let Some(v) = voice {
            cmd.arg("-v").arg(v);
        }
        cmd.arg("-f").arg("-");
        if !run_with_stdin(cmd, text, stop).await? {
            return Ok(Spoken::Native);
        }
    } else if let Some(espeak) = which("espeak-ng") {
        let mut cmd = tokio::process::Command::new(espeak);
        cmd.arg("-w")
            .arg(&tmp)
            .arg("-s")
            .arg(espeak_wpm(rate).to_string())
            .arg("-p")
            .arg(espeak_pitch(pitch).to_string());
        if let Some(v) = voice {
            cmd.arg("-v").arg(v);
        }
        cmd.arg("--stdin");
        if !run_with_stdin(cmd, text, stop).await? {
            return Ok(Spoken::Native);
        }
    } else if let Some(spd) = which("spd-say") {
        let mut cmd = tokio::process::Command::new(spd);
        cmd.arg("-w")
            .arg("-r")
            .arg(offset_of(rate).to_string())
            .arg("-p")
            .arg(offset_of(pitch).to_string());
        if let Some(v) = voice {
            cmd.arg("-y").arg(v);
        }
        cmd.arg("--").arg(text);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(err)?;
        let stopped = stop.notified();
        tokio::pin!(stopped);
        tokio::select! {
            status = child.wait() => {
                let status = status.map_err(err)?;
                if !status.success() {
                    return Err(err(format!("spd-say failed ({status})")));
                }
            }
            _ = &mut stopped => {
                let _ = child.kill().await;
                // speech-dispatcher keeps speaking the queued message unless told to stop.
                let _ = tokio::process::Command::new("spd-say").arg("-S").status().await;
            }
        }
        return Ok(Spoken::Native);
    } else {
        return Err(no_voice());
    }
    tokio::fs::rename(&tmp, target).await.map_err(err)?;
    Ok(Spoken::File)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_and_pitches() {
        assert_eq!(espeak_wpm(100), 175);
        assert_eq!(espeak_wpm(200), 350);
        assert_eq!(espeak_wpm(10), 87);
        assert_eq!(espeak_pitch(100), 50);
        assert_eq!(espeak_pitch(200), 99);
        assert_eq!(offset_of(150), 50);
        assert_eq!(offset_of(50), -50);
    }

    #[test]
    fn parses_voice_listings() {
        let espeak = "Pty Language       Age/Gender VoiceName          File                 Other Languages\n \
                      5  es              --/M      Spanish_(Spain)    roa/es\n \
                      5  en-us           --/F      English_(America)  gmw/en-US            (en 3)\n";
        let v = parse_espeak_voices(espeak);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "es");
        assert_eq!(v[0].gender.as_deref(), Some("M"));
        assert_eq!(v[1].name, "English (America) (en-us)");
        let spd = parse_spd_voices("     NAME     LANGUAGE     VARIANT\n  Paulina   es-MX   none\n");
        assert_eq!(spd.len(), 1);
        assert_eq!(spd[0].id, "Paulina");
        let say = parse_say_voices(
            "Bad News            en_US    # The light you see\nMonica              es_ES    # Hola\n",
        );
        assert_eq!(say[0].id, "Bad News");
        assert_eq!(say[1].lang.as_deref(), Some("es-ES"));
        let sapi = parse_sapi_voices("Microsoft Sabina Desktop|es-MX|Female\n\nX||NotSet\n");
        assert_eq!(sapi.len(), 2);
        assert_eq!(sapi[0].gender.as_deref(), Some("Female"));
        assert_eq!(sapi[1].gender, None);
    }
}
