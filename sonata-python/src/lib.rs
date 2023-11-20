use once_cell::sync::OnceCell;
use sonata_core::{SonataError, SonataModel, Audio, AudioInfo, AudioSamples};
use sonata_synth::{
    AudioOutputConfig, SonataSpeechStreamLazy, SonataSpeechStreamParallel,
    SonataSpeechSynthesizer, SYNTHESIS_THREAD_POOL,
};
use sonata_piper::PiperSynthesisConfig;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

type PySonataResult<T> = Result<T, PySonataError>;
static ORT_ENVIRONMENT: OnceCell<Arc<ort::Environment>> = OnceCell::new();

create_exception!(
    piper,
    SonataException,
    PyException,
    "Base Exception for all exceptions raised by piper."
);

struct PySonataError(SonataError);

impl From<PySonataError> for PyErr {
    fn from(other: PySonataError) -> Self {
        SonataException::new_err(other.0.to_string())
    }
}

impl From<SonataError> for PySonataError {
    fn from(other: SonataError) -> Self {
        Self(other)
    }
}

#[pyclass(weakref, module = "piper", frozen)]
#[pyo3(name = "AudioInfo")]
struct PyWaveInfo(AudioInfo);

#[pymethods]
impl PyWaveInfo {
    #[getter]
    fn get_sample_rate(&self) -> usize {
        self.0.sample_rate
    }
    #[getter]
    fn get_num_channels(&self) -> usize {
        self.0.num_channels
    }
    #[getter]
    fn get_sample_width(&self) -> usize {
        self.0.sample_width
    }
}

impl From<AudioInfo> for PyWaveInfo {
    fn from(other: AudioInfo) -> Self {
        Self(other)
    }
}

#[pyclass(weakref, module = "piper", frozen)]
#[pyo3(name = "AudioOutputConfig")]
#[derive(Clone)]
struct PyAudioOutputConfig(AudioOutputConfig);

#[pymethods]
impl PyAudioOutputConfig {
    #[new]
    fn new(
        rate: Option<u8>,
        volume: Option<u8>,
        pitch: Option<u8>,
        appended_silence_ms: Option<u32>,
    ) -> Self {
        Self(AudioOutputConfig {
            rate,
            volume,
            pitch,
            appended_silence_ms,
        })
    }
}

impl From<PyAudioOutputConfig> for AudioOutputConfig {
    fn from(other: PyAudioOutputConfig) -> Self {
        other.0
    }
}

#[pyclass(weakref, module = "piper", frozen)]
struct WaveSamples(Audio);

#[pymethods]
impl WaveSamples {
    fn get_wave_bytes(&self, py: Python) -> PyObject {
        let bytes_vec = py.allow_threads(move || self.0.as_wave_bytes());
        PyBytes::new(py, &bytes_vec).into()
    }
    fn save_to_file(&self, filename: &str) -> PySonataResult<()> {
        Ok(self.0.save_to_file(filename).map_err(|e| SonataError::from(e))?)
    }
    #[getter]
    fn sample_rate(&self) -> usize {
        self.0.info.sample_rate
    }
    #[getter]
    fn num_channels(&self) -> usize {
        self.0.info.num_channels
    }
    #[getter]
    fn sample_width(&self) -> usize {
        self.0.info.sample_width
    }
    #[getter]
    fn inference_ms(&self) -> Option<f32> {
        self.0.inference_ms()
    }
    #[getter]
    fn duration_ms(&self) -> f32 {
        self.0.duration_ms()
    }
    #[getter]
    fn real_time_factor(&self) -> Option<f32> {
        self.0.real_time_factor()
    }
}

#[pyclass(weakref, module = "piper")]
struct LazySpeechStream(SonataSpeechStreamLazy);

impl From<SonataSpeechStreamLazy> for LazySpeechStream {
    fn from(other: SonataSpeechStreamLazy) -> Self {
        Self(other)
    }
}

#[pymethods]
impl LazySpeechStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> Option<WaveSamples> {
        let next_item = py.allow_threads(|| self.0.next());
        let audio_result = match next_item {
            Some(result) => result,
            None => return None,
        };
        match audio_result {
            Ok(audio_data) => Some(WaveSamples(audio_data)),
            Err(e) => {
                PyErr::from(PySonataError::from(e)).restore(py);
                None
            }
        }
    }
}

#[pyclass(weakref, module = "piper")]
struct ParallelSpeechStream(SonataSpeechStreamParallel);

impl From<SonataSpeechStreamParallel> for ParallelSpeechStream {
    fn from(other: SonataSpeechStreamParallel) -> Self {
        Self(other)
    }
}

#[pymethods]
impl ParallelSpeechStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> Option<WaveSamples> {
        let next_item = py.allow_threads(|| self.0.next());
        let audio_result = match next_item {
            Some(result) => result,
            None => return None,
        };
        match audio_result {
            Ok(audio_data) => Some(WaveSamples(audio_data)),
            Err(e) => {
                PyErr::from(PySonataError::from(e)).restore(py);
                None
            }
        }
    }
}

#[pyclass(weakref, module = "piper")]
struct RealtimeSpeechStream(std::sync::mpsc::IntoIter<PySonataResult<AudioSamples>>);

impl RealtimeSpeechStream {
    fn new(receiver: std::sync::mpsc::Receiver<PySonataResult<AudioSamples>>) -> Self {
        Self(receiver.into_iter())
    }
}

#[pymethods]
impl RealtimeSpeechStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python) -> Option<PyObject> {
        let result = py.allow_threads(|| self.0.next())?;
        match result {
            Ok(samples) => Some(PyBytes::new(py, &samples.as_wave_bytes()).into()),
            Err(e) => {
                PyErr::from(e).restore(py);
                None
            }
        }
    }
}

#[pyclass(weakref, module = "piper")]
struct PiperScales {
    #[allow(dead_code)]
    length_scale: f32,
    #[allow(dead_code)]
    noise_scale: f32,
    #[allow(dead_code)]
    noise_w: f32,
}

#[pymethods]
impl PiperScales {
    #[new]
    fn new(length_scale: f32, noise_scale: f32, noise_w: f32) -> PySonataResult<Self> {
        Ok(Self {
            length_scale,
            noise_scale,
            noise_w,
        })
    }
}

#[pyclass(weakref, module = "piper")]
#[pyo3(name = "PiperModel")]
struct PiperModel(Arc<dyn SonataModel + Send + Sync>);

impl PiperModel {
    fn get_ort_environment() -> &'static Arc<ort::Environment> {
        ORT_ENVIRONMENT.get_or_init(|| {
            Arc::new(
                ort::Environment::builder()
                    .with_name("piper")
                    .with_execution_providers([ort::ExecutionProvider::CPU(Default::default())])
                    .build()
                    .unwrap(),
            )
        })
    }
}

#[pymethods]
impl PiperModel {
    #[new]
    fn new(config_path: &str) -> PySonataResult<Self> {
        let vits =
            sonata_piper::from_config_path(&PathBuf::from(config_path), Self::get_ort_environment())?;
        Ok(Self(vits))
    }
    #[getter]
    fn get_speaker(&self) -> PySonataResult<Option<String>> {
        match self
            .0
            .get_fallback_synthesis_config()?
            .downcast_ref::<PiperSynthesisConfig>()
        {
            Some(synth_config) => match synth_config.speaker {
                Some(sid) => Ok(self.0.speaker_id_to_name(&sid)?),
                None => Ok(None),
            },
            None => Ok(None),
        }
    }
    #[setter]
    fn set_speaker(&self, name: String) -> PySonataResult<()> {
        let sid = match self.0.speaker_name_to_id(&name)? {
            Some(sname) => sname,
            None => {
                return Err(SonataError::OperationError(format!(
                    "A speaker with the given name `{}` was not found",
                    name
                ))
                .into())
            }
        };
        match self
            .0
            .get_fallback_synthesis_config()?
            .downcast::<PiperSynthesisConfig>()
        {
            Ok(mut synth_config) => {
                synth_config.speaker = Some(sid);
                Ok(self.0.set_fallback_synthesis_config(&synth_config)?)
            }
            Err(_) => {
                Err(SonataError::OperationError("Cannot set synthesis config".to_string()).into())
            }
        }
    }
    fn get_scales(&self) -> PySonataResult<PiperScales> {
        match self
            .0
            .get_fallback_synthesis_config()?
            .downcast::<PiperSynthesisConfig>()
        {
            Ok(synth_config) => Ok(PiperScales {
                length_scale: synth_config.length_scale,
                noise_scale: synth_config.noise_scale,
                noise_w: synth_config.noise_w,
            }),
            Err(_) => {
                Err(SonataError::OperationError("Cannot set synthesis config".to_string()).into())
            }
        }
    }
    fn set_scales(&self, length_scale: f32, noise_scale: f32, noise_w: f32) -> PySonataResult<()> {
        match self
            .0
            .get_fallback_synthesis_config()?
            .downcast::<PiperSynthesisConfig>()
        {
            Ok(mut synth_config) => {
                synth_config.length_scale = length_scale;
                synth_config.noise_scale = noise_scale;
                synth_config.noise_w = noise_w;
                Ok(self.0.set_fallback_synthesis_config(&synth_config)?)
            }
            Err(_) => {
                Err(SonataError::OperationError("Cannot set synthesis config".to_string()).into())
            }
        }
    }
}

#[pyclass(weakref, module = "piper", frozen)]
struct Sonata(Arc<SonataSpeechSynthesizer>);

#[pymethods]
impl Sonata {
    #[staticmethod]
    fn with_piper(vits_model: &PiperModel) -> PySonataResult<Self> {
        let model = Arc::clone(&vits_model.0);
        let synthesizer = Arc::new(SonataSpeechSynthesizer::new(model)?);
        Ok(Self(synthesizer))
    }
    fn synthesize(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PySonataResult<LazySpeechStream> {
        self.synthesize_lazy(text, audio_output_config)
    }

    fn synthesize_lazy(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PySonataResult<LazySpeechStream> {
        Ok(self
            .0
            .synthesize_lazy(text, audio_output_config.map(|o| o.into()))?
            .into())
    }

    fn synthesize_parallel(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PySonataResult<ParallelSpeechStream> {
        Ok(self
            .0
            .synthesize_parallel(text, audio_output_config.map(|o| o.into()))?
            .into())
    }

    fn synthesize_streamed(
        &self,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
        chunk_size: Option<usize>,
        chunk_padding: Option<usize>,
    ) -> PySonataResult<RealtimeSpeechStream> {
        let synth = Arc::clone(&self.0);
        let (tx, rx) = std::sync::mpsc::channel::<PySonataResult<AudioSamples>>();
        SYNTHESIS_THREAD_POOL.spawn_fifo(move || {
            let stream_result = synth.synthesize_streamed(
                text,
                audio_output_config.map(|o| o.into()),
                chunk_size.unwrap_or(45),
                chunk_padding.unwrap_or(3),
            );
            let stream = match stream_result {
                Ok(stream) => stream,
                Err(e) => {
                    tx.send(Err(e.into())).unwrap();
                    return;
                }
            };
            for result in stream {
                let samples: PySonataResult<AudioSamples> = result.map_err(|e| e.into());
                tx.send(samples).unwrap();
            }
        });
        Ok(RealtimeSpeechStream::new(rx))
    }

    fn synthesize_to_file(
        &self,
        filename: &str,
        text: String,
        audio_output_config: Option<PyAudioOutputConfig>,
    ) -> PySonataResult<()> {
        self.0
            .synthesize_to_file(filename, text, audio_output_config.map(|o| o.into()))?;
        Ok(())
    }
    #[getter]
    fn language(&self) -> PySonataResult<Option<String>> {
        Ok(self.0.get_language()?)
    }
    #[getter]
    fn speakers(&self) -> PySonataResult<Option<HashMap<i64, String>>> {
        Ok(self.0.get_speakers()?.cloned())
    }
    fn get_audio_output_info(&self) -> PySonataResult<PyWaveInfo> {
        Ok(self.0.audio_output_info()?.into())
    }
}

/// A fast, local neural text-to-speech engine
#[pymodule]
fn sonata(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add("SonataException", _py.get_type::<SonataException>())?;
    m.add_class::<Sonata>()?;
    m.add_class::<PiperModel>()?;
    m.add_class::<PiperScales>()?;
    m.add_class::<PyAudioOutputConfig>()?;
    m.add_class::<WaveSamples>()?;
    m.add_class::<LazySpeechStream>()?;
    m.add_class::<ParallelSpeechStream>()?;
    m.add_class::<RealtimeSpeechStream>()?;
    Ok(())
}