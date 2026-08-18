//! Costruzione del passaggio ffmpeg che misura VMAF sull'output definitivo.

use std::ffi::OsString;
use std::path::Path;

use anyhow::{bail, Result};

use crate::config::EncodingParams;
use crate::ffmpeg::encode::video_encoder_args;
use crate::models::{MotivoScelta, RisultatoCrf, VmafAnalysis};

/// Durata fissa di ogni spezzone sondato. Non configurabile: la leva offerta
/// all'utente è il numero di segmenti.
pub const DURATA_SEGMENTO_SECONDI: f64 = 30.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentoAnalisi {
    pub indice: u32,
    pub inizio_secondi: f64,
    pub durata_secondi: f64,
}

/// Spezzoni da sondare: casuali, non sovrapposti, con il primo e l'ultimo 5%
/// della durata esclusi perché lì stanno sigle, nero e titoli di coda.
pub fn segmenti_da_analizzare(
    durata_secondi: f64,
    richiesti: u32,
    entropia: &[u8; 16],
) -> Vec<SegmentoAnalisi> {
    if durata_secondi <= DURATA_SEGMENTO_SECONDI {
        return vec![SegmentoAnalisi {
            indice: 0,
            inizio_secondi: 0.0,
            durata_secondi,
        }];
    }

    let inizio_utile = durata_secondi * 0.05;
    let fine_utile = durata_secondi * 0.95;
    let finestra = fine_utile - inizio_utile;
    if finestra < DURATA_SEGMENTO_SECONDI {
        return vec![SegmentoAnalisi {
            indice: 0,
            inizio_secondi: inizio_utile,
            durata_secondi: DURATA_SEGMENTO_SECONDI.min(durata_secondi - inizio_utile),
        }];
    }

    let massimo = (finestra / DURATA_SEGMENTO_SECONDI).floor() as u32;
    let n = richiesti.min(massimo).max(1);
    let blocco = finestra / n as f64;
    (0..n)
        .map(|i| {
            let margine = (blocco - DURATA_SEGMENTO_SECONDI).max(0.0);
            let offset = margine * (entropia[i as usize % 16] as f64 / 255.0);
            let inizio = inizio_utile + i as f64 * blocco + offset;
            SegmentoAnalisi {
                indice: i,
                inizio_secondi: inizio,
                durata_secondi: DURATA_SEGMENTO_SECONDI.min(fine_utile - inizio),
            }
        })
        .collect()
}

/// CRF candidati: `crf+2`, `crf+4`, … per `step` valori. I valori sopra 63
/// vengono scartati, perché è il massimo accettato da SVT-AV1.
pub fn crf_da_testare(crf_di_partenza: u8, step: u32) -> Vec<u8> {
    (1..=step)
        .filter_map(|indice| {
            let crf = crf_di_partenza as u16 + indice as u16 * 2;
            (crf <= 63).then_some(crf as u8)
        })
        .collect()
}

pub struct SondaRequest<'a> {
    pub params: &'a EncodingParams,
    pub input: &'a Path,
    pub output_mkv: &'a Path,
    pub segmento: &'a SegmentoAnalisi,
    pub crf: u8,
}

/// Codifica di prova di un solo spezzone: nessun audio, contenitore Matroska,
/// stessi parametri video della codifica finale.
pub fn build_probe_args(request: &SondaRequest<'_>) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostats".into(),
        "-stats_period".into(),
        request.params.progress_interval.to_string().into(),
        "-progress".into(),
        "pipe:1".into(),
        "-y".into(),
        "-ss".into(),
        request.segmento.inizio_secondi.to_string().into(),
        "-t".into(),
        request.segmento.durata_secondi.to_string().into(),
        "-i".into(),
        request.input.as_os_str().to_owned(),
        "-map".into(),
        "0:v:0".into(),
        "-an".into(),
    ];
    args.extend(video_encoder_args(request.params));
    args.extend([
        "-b:v".into(),
        "0".into(),
        "-crf".into(),
        request.crf.to_string().into(),
        "-f".into(),
        "matroska".into(),
        request.output_mkv.as_os_str().to_owned(),
    ]);
    args
}

pub struct VmafRequest<'a> {
    /// File convertito da valutare.
    pub distorted: &'a Path,
    /// Sorgente usato come riferimento.
    pub reference: &'a Path,
    /// Percorso del log JSON prodotto da libvmaf.
    pub log_json: &'a Path,
    pub model: &'a str,
    pub pixel_format: &'a str,
    pub threads: Option<u32>,
    /// Inizio del riferimento da confrontare, prima del secondo input.
    pub riferimento_inizio: Option<f64>,
    /// Durata del riferimento da confrontare, prima del secondo input.
    pub riferimento_durata: Option<f64>,
    /// Geometria del distorto, presente solo quando differisce da quella del
    /// sorgente: libvmaf confronta solo immagini della stessa dimensione.
    pub scale_to: Option<(u32, u32)>,
    pub progress_interval: f64,
}

/// Costruisce gli argomenti per confrontare output definitivo e sorgente.
pub fn build_vmaf_args(request: &VmafRequest<'_>) -> Vec<OsString> {
    let mut reference_branch = String::from("[1:v]settb=AVTB,setpts=PTS-STARTPTS");
    if let Some((width, height)) = request.scale_to {
        // La riscalatura del riferimento serve solo quando l'output ha una
        // geometria diversa; reset_sar replica il filtro di codifica.
        reference_branch.push_str(&format!(
            ",scale=w={width}:h={height}:flags=bicubic:reset_sar=1"
        ));
    }
    // Le basi tempi vengono allineate perché MP4 e sorgente possono usare timebase diverse.
    // format porta entrambi i rami a profondità e formato accettati da libvmaf.
    reference_branch.push_str(&format!(",format={}[ref]", request.pixel_format));
    let mut filter = format!(
        "[0:v]settb=AVTB,setpts=PTS-STARTPTS,format={}[dist];{};[dist][ref]libvmaf=model=version={}:log_fmt=json:log_path={}",
        request.pixel_format,
        reference_branch,
        request.model,
        request.log_json.display()
    );
    if let Some(threads) = request.threads {
        filter.push_str(&format!(":n_threads={threads}"));
    }

    let mut args = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostats".into(),
        "-stats_period".into(),
        request.progress_interval.to_string().into(),
        "-progress".into(),
        "pipe:1".into(),
        "-y".into(),
        "-i".into(),
        request.distorted.as_os_str().to_owned(),
    ];
    if let Some(inizio) = request.riferimento_inizio {
        args.push("-ss".into());
        args.push(inizio.to_string().into());
    }
    if let Some(durata) = request.riferimento_durata {
        args.push("-t".into());
        args.push(durata.to_string().into());
    }
    args.extend([
        "-i".into(),
        request.reference.as_os_str().to_owned(),
        "-lavfi".into(),
        filter.into(),
        "-an".into(),
        "-f".into(),
        "null".into(),
        if cfg!(windows) {
            "NUL".into()
        } else {
            "/dev/null".into()
        },
    ]);
    args
}

/// Applica il criterio: fra i CRF con misura valida si prende il più alto con
/// VMAF medio >= target. Senza candidati si torna al CRF impostato dall'utente.
pub fn scegli_crf(
    risultati: &[RisultatoCrf],
    target: f64,
    crf_di_partenza: u8,
) -> (u8, MotivoScelta) {
    let validi: Vec<&RisultatoCrf> = risultati
        .iter()
        .filter(|risultato| risultato.vmaf_medio.is_some())
        .collect();
    if validi.is_empty() {
        return (crf_di_partenza, MotivoScelta::AnalisiNonEseguibile);
    }
    let candidati: Vec<&&RisultatoCrf> = validi
        .iter()
        .filter(|risultato| risultato.vmaf_medio.unwrap_or_default() >= target)
        .collect();
    let Some(scelto) = candidati.iter().max_by_key(|risultato| risultato.crf) else {
        return (crf_di_partenza, MotivoScelta::NessunoSopraSoglia);
    };
    let motivo = if candidati.len() == validi.len() {
        MotivoScelta::TuttiSopraSoglia
    } else {
        MotivoScelta::SopraSoglia
    };
    (scelto.crf, motivo)
}

/// Righe di riepilogo dell'analisi, una per CRF più la conclusione.
pub fn righe_riepilogo(analisi: &VmafAnalysis) -> Vec<String> {
    let mut righe = Vec::with_capacity(analisi.risultati.len() + 2);
    for risultato in &analisi.risultati {
        let vmaf = risultato.vmaf_medio.map_or_else(
            || "non disponibile".to_string(),
            |vmaf| format!("{vmaf:.2}"),
        );
        let bitrate = risultato.bitrate_medio_bps.map_or_else(
            || "bitrate non disponibile".to_string(),
            |bitrate| format!("{:.2} Mb/s", bitrate as f64 / 1_000_000.0),
        );
        righe.push(format!(
            "Analisi VMAF - CRF {}: VMAF medio {} su {} segmenti, {}",
            risultato.crf,
            vmaf,
            risultato.vmaf_per_segmento.len(),
            bitrate
        ));
    }
    let vmaf_scelto = analisi
        .risultati
        .iter()
        .find(|risultato| risultato.crf == analisi.crf_scelto)
        .and_then(|risultato| risultato.vmaf_medio)
        .map_or_else(
            || "non disponibile".to_string(),
            |vmaf| format!("{vmaf:.2}"),
        );
    let conclusione = match analisi.motivo {
        MotivoScelta::NessunoSopraSoglia | MotivoScelta::AnalisiNonEseguibile => format!(
            "Analisi VMAF - CRF scelto: {} (VMAF medio {}, target {:.2}, modello {}): si usa il CRF impostato dall'utente",
            analisi.crf_scelto, vmaf_scelto, analisi.target, analisi.modello
        ),
        _ => format!(
        "Analisi VMAF - CRF scelto: {} (VMAF medio {}, target {:.2}, modello {})",
        analisi.crf_scelto, vmaf_scelto, analisi.target, analisi.modello
        ),
    };
    righe.push(conclusione);
    if analisi.motivo == MotivoScelta::TuttiSopraSoglia {
        righe.push(
            "Analisi VMAF - tutti i CRF provati restano sopra il target: aumentare \"CRF Test Steps\" per spingere oltre".to_string(),
        );
    }
    righe
}

#[derive(Debug, Clone, PartialEq)]
pub struct VmafScores {
    pub mean: Option<f64>,
    pub min: Option<f64>,
    pub harmonic_mean: Option<f64>,
}

/// Legge le metriche aggregate dal log JSON di libvmaf.
pub fn parse_vmaf_log(contenuto: &str) -> Result<VmafScores> {
    let json: serde_json::Value = serde_json::from_str(contenuto)?;
    let Some(vmaf) = json
        .get("pooled_metrics")
        .and_then(|pooled| pooled.get("vmaf"))
    else {
        bail!("log VMAF senza metriche aggregate");
    };
    Ok(VmafScores {
        mean: vmaf.get("mean").and_then(serde_json::Value::as_f64),
        min: vmaf.get("min").and_then(serde_json::Value::as_f64),
        harmonic_mean: vmaf
            .get("harmonic_mean")
            .and_then(serde_json::Value::as_f64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(scale_to: Option<(u32, u32)>, threads: Option<u32>) -> VmafRequest<'static> {
        VmafRequest {
            distorted: Path::new("out.mp4"),
            reference: Path::new("source.mkv"),
            log_json: Path::new("vmaf.json"),
            model: "vmaf_v1.0.16_5d0h",
            pixel_format: "yuv420p10le",
            threads,
            riferimento_inizio: None,
            riferimento_durata: None,
            scale_to,
            progress_interval: 5.0,
        }
    }

    fn filter(args: &[OsString]) -> String {
        let index = args.iter().position(|arg| arg == "-lavfi").unwrap() + 1;
        args[index].to_string_lossy().into_owned()
    }

    #[test]
    fn gli_argomenti_vmaf_contengono_i_due_input_e_il_filtro() {
        let args = build_vmaf_args(&request(None, None));
        assert_eq!(args.iter().filter(|arg| *arg == "-i").count(), 2);
        assert!(args.contains(&OsString::from("-lavfi")));
        assert!(args.contains(&OsString::from("-f")));
        assert_eq!(
            args.last().unwrap(),
            if cfg!(windows) { "NUL" } else { "/dev/null" }
        );
    }

    #[test]
    fn il_riferimento_non_viene_riscalato_se_la_geometria_coincide() {
        assert!(!filter(&build_vmaf_args(&request(None, None))).contains("scale="));
    }

    #[test]
    fn il_riferimento_viene_riscalato_alla_geometria_del_distorto() {
        assert!(filter(&build_vmaf_args(&request(Some((1280, 720)), None)))
            .contains("scale=w=1280:h=720:flags=bicubic:reset_sar=1"));
    }

    #[test]
    fn il_modello_e_il_log_finiscono_nel_filtro_libvmaf() {
        let filter = filter(&build_vmaf_args(&request(None, None)));
        assert!(filter.contains("format=yuv420p10le[ref];[dist][ref]libvmaf"));
        assert!(filter.contains("libvmaf=model=version=vmaf_v1.0.16_5d0h"));
        assert!(filter.contains("log_fmt=json"));
        assert!(filter.contains("log_path=vmaf.json"));
    }

    #[test]
    fn i_thread_vmaf_sono_opzionali() {
        assert!(!filter(&build_vmaf_args(&request(None, None))).contains("n_threads"));
        assert!(filter(&build_vmaf_args(&request(None, Some(4)))).contains("n_threads=4"));
    }

    #[test]
    fn il_log_json_di_vmaf_viene_interpretato() {
        let scores = parse_vmaf_log(
            r#"{"pooled_metrics":{"vmaf":{"min":80.1,"max":99.0,"mean":95.5,"harmonic_mean":95.0}}}"#,
        )
        .unwrap();
        assert_eq!(scores.mean, Some(95.5));
        assert_eq!(scores.min, Some(80.1));
        assert_eq!(scores.harmonic_mean, Some(95.0));
    }

    #[test]
    fn un_log_json_senza_metriche_da_errore() {
        assert!(parse_vmaf_log(r#"{"frames":[]}"#).is_err());
    }

    #[test]
    fn i_crf_da_testare_crescono_di_due() {
        assert_eq!(crf_da_testare(40, 3), vec![42, 44, 46]);
    }

    #[test]
    fn i_crf_oltre_il_massimo_vengono_scartati() {
        assert_eq!(crf_da_testare(60, 5), vec![62]);
        assert!(crf_da_testare(63, 2).is_empty());
    }

    #[test]
    fn i_segmenti_escludono_testa_e_coda() {
        let segmenti = segmenti_da_analizzare(600.0, 3, &[0; 16]);
        assert_eq!(segmenti.len(), 3);
        assert!(segmenti[0].inizio_secondi >= 30.0);
        let ultimo = segmenti.last().unwrap();
        assert!(ultimo.inizio_secondi + ultimo.durata_secondi <= 570.0);
    }

    #[test]
    fn i_segmenti_non_si_sovrappongono() {
        let segmenti = segmenti_da_analizzare(600.0, 3, &[127; 16]);
        for coppia in segmenti.windows(2) {
            assert!(
                coppia[1].inizio_secondi >= coppia[0].inizio_secondi + coppia[0].durata_secondi
            );
        }
    }

    #[test]
    fn il_numero_di_segmenti_si_riduce_se_la_durata_non_basta() {
        assert!(segmenti_da_analizzare(100.0, 5, &[0; 16]).len() <= 3);
    }

    #[test]
    fn un_file_corto_diventa_un_unico_segmento() {
        assert_eq!(
            segmenti_da_analizzare(10.0, 5, &[0; 16]),
            vec![SegmentoAnalisi {
                indice: 0,
                inizio_secondi: 0.0,
                durata_secondi: 10.0,
            }]
        );
    }

    #[test]
    fn gli_stessi_byte_danno_gli_stessi_segmenti() {
        let primi = segmenti_da_analizzare(600.0, 3, &[1; 16]);
        assert_eq!(primi, segmenti_da_analizzare(600.0, 3, &[1; 16]));
        assert_ne!(primi, segmenti_da_analizzare(600.0, 3, &[2; 16]));
    }

    fn probe_request() -> SondaRequest<'static> {
        SondaRequest {
            params: Box::leak(Box::new(EncodingParams::default())),
            input: Path::new("source.mp4"),
            output_mkv: Path::new("probe.mkv"),
            segmento: Box::leak(Box::new(SegmentoAnalisi {
                indice: 1,
                inizio_secondi: 120.0,
                durata_secondi: 30.0,
            })),
            crf: 44,
        }
    }

    #[test]
    fn gli_argomenti_della_sonda_tagliano_lo_spezzone() {
        let args = build_probe_args(&probe_request());
        assert!(args
            .windows(2)
            .any(|pair| pair == [OsString::from("-ss"), OsString::from("120")]));
        assert!(args
            .windows(2)
            .any(|pair| pair == [OsString::from("-t"), OsString::from("30")]));
        assert!(args.contains(&OsString::from("-an")));
        assert!(args
            .windows(2)
            .any(|pair| pair == [OsString::from("-crf"), OsString::from("44")]));
        assert_eq!(args.last().unwrap(), "probe.mkv");
        assert!(args.contains(&OsString::from("-vf")));
    }

    #[test]
    fn gli_argomenti_di_misura_tagliano_il_riferimento() {
        let mut request = request(None, None);
        request.riferimento_inizio = Some(120.0);
        request.riferimento_durata = Some(30.0);
        let args = build_vmaf_args(&request);
        let primo_input = args.iter().position(|arg| arg == "-i").unwrap();
        let secondo_input = args
            .iter()
            .skip(primo_input + 1)
            .position(|arg| arg == "-i")
            .unwrap()
            + primo_input
            + 1;
        let taglio = args[..secondo_input]
            .windows(2)
            .position(|pair| pair == [OsString::from("-ss"), OsString::from("120")]);
        assert!(taglio.is_some());
        assert!(args[..secondo_input]
            .windows(2)
            .any(|pair| pair == [OsString::from("-t"), OsString::from("30")]));
        assert!(!args[..args.iter().position(|arg| arg == "-i").unwrap()]
            .contains(&OsString::from("-ss")));
    }

    fn risultato(crf: u8, vmaf: Option<f64>) -> RisultatoCrf {
        RisultatoCrf {
            crf,
            vmaf_medio: vmaf,
            bitrate_medio_bps: Some(1_000_000),
            vmaf_per_segmento: vec![vmaf],
        }
    }

    #[test]
    fn il_criterio_prende_il_crf_piu_alto_sopra_soglia() {
        assert_eq!(
            scegli_crf(
                &[
                    risultato(42, Some(96.8)),
                    risultato(44, Some(95.1)),
                    risultato(46, Some(91.0))
                ],
                93.0,
                40
            ),
            (44, MotivoScelta::SopraSoglia)
        );
    }

    #[test]
    fn il_criterio_segnala_quando_tutti_superano_la_soglia() {
        assert_eq!(
            scegli_crf(
                &[risultato(42, Some(96.8)), risultato(44, Some(95.1))],
                93.0,
                40
            ),
            (44, MotivoScelta::TuttiSopraSoglia)
        );
    }

    #[test]
    fn il_criterio_ripiega_sul_crf_impostato() {
        assert_eq!(
            scegli_crf(&[risultato(42, Some(90.0))], 93.0, 40),
            (40, MotivoScelta::NessunoSopraSoglia)
        );
        assert_eq!(
            scegli_crf(&[risultato(42, None)], 93.0, 40),
            (40, MotivoScelta::AnalisiNonEseguibile)
        );
    }

    #[test]
    fn il_riepilogo_dichiara_il_crf_scelto() {
        let analisi = VmafAnalysis {
            crf_scelto: 44,
            crf_di_partenza: 40,
            target: 93.0,
            modello: "vmaf_v1.0.16_5d0h".into(),
            motivo: MotivoScelta::SopraSoglia,
            segmenti: Vec::new(),
            risultati: vec![risultato(42, Some(96.8)), risultato(44, Some(95.1))],
            duration_seconds: 1.0,
            error: None,
        };
        let righe = righe_riepilogo(&analisi);
        assert_eq!(righe.len(), 3);
        assert!(righe[2].starts_with("Analisi VMAF - CRF scelto:"));
    }

    #[test]
    fn un_log_senza_media_vmaf_restituisce_media_assente() {
        let scores = parse_vmaf_log(r#"{"pooled_metrics":{"vmaf":{"min":80.1}}}"#).unwrap();
        assert_eq!(scores.mean, None);
    }
}
