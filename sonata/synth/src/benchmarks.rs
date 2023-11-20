mod dev_utils;
use sonata_synth::{SonataSpeechSynthesizer, AudioOutputConfig};


fn main() {
    dev_utils::init();
    divan::main();
}


fn provide_params(kind: &'static str) ->  impl Fn() -> (SonataSpeechSynthesizer, String, Option<AudioOutputConfig>) {
    move || dev_utils::gen_params(kind)
}

#[divan::bench_group(sample_count=5, sample_size=3)]
mod speech_streams {
    use super::*;
    use divan::{Bencher, black_box};

    #[divan::bench(threads=4)]
    fn bench_lazy_stream(bencher: Bencher) {
        bencher
            .with_inputs(provide_params("std"))
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth.synthesize_lazy(text.clone(), output_config.clone())
                    .unwrap()
                    .map(|res| res.map(|a| a.samples));
                dev_utils::iterate_stream(black_box(stream)).unwrap();
            });
    }

    #[divan::bench]
    fn bench_parallel_stream(bencher: Bencher) {
        bencher
            .with_inputs(provide_params("std"))
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth.synthesize_parallel(text.clone(), output_config.clone())
                    .unwrap()
                    .map(|res| res.map(|a| a.samples));
                dev_utils::iterate_stream(black_box(stream)).unwrap();
            });
    }

    #[divan::bench]
    fn bench_realtime_stream(bencher: Bencher) {
        bencher
            .with_inputs(provide_params("rt"))
            .bench_local_refs(|(synth, text, output_config)| {
                let stream = synth.synthesize_streamed(text.clone(), output_config.clone(), 72, 3)
                    .unwrap();
                    dev_utils::iterate_stream(black_box(stream)).unwrap();
            });
    }
}
