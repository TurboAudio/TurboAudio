use dasp::Sample;
use dasp_signal::Signal;
use dasp_window::Window;
use rustfft::{num_complex::Complex, num_traits::ToPrimitive};
use std::sync::{Arc, RwLock};

#[derive(Default)]
pub struct FftResult {
    pub raw_bins: Vec<f32>,
}

const SAMPLE_RATE: usize = 48000;
const FFT_SIZE: usize = 1024;
const FFT_RESOLUTION: f32 = SAMPLE_RATE as f32 / FFT_SIZE as f32;

impl FftResult {
    pub fn new(raw_bins: Vec<f32>) -> Self {
        Self { raw_bins }
    }

    pub fn get_low_frequency_amplitude(&self) -> f32 {
        let (min_freq, max_freq): (usize, usize) = (0, 100);
        self.get_frequency_interval_average_amplitude(&min_freq, &max_freq)
            .unwrap_or(0.0)
    }

    pub fn get_mid_frequency_amplitude(&self) -> f32 {
        let (min_freq, max_freq): (usize, usize) = (100, 1000);
        self.get_frequency_interval_average_amplitude(&min_freq, &max_freq)
            .unwrap_or(0.0)
    }

    pub fn get_high_frequency_amplitude(&self) -> f32 {
        let (min_freq, max_freq): (usize, usize) = (1000, 2000);
        self.get_frequency_interval_average_amplitude(&min_freq, &max_freq)
            .unwrap_or(0.0)
    }

    pub fn get_frequency_interval_average_amplitude(
        &self,
        min_freq: &usize,
        max_freq: &usize,
    ) -> Option<f32> {
        let sum: f32 = (*min_freq..*max_freq)
            .map(|frequency| self.get_frequency_amplitude(&frequency).unwrap_or(0.0))
            .sum();
        let interval_size = (max_freq - min_freq).to_f32()?;
        Some(sum / interval_size)
    }

    pub fn get_frequency_interval_average(&self, low: usize, high: usize) -> f32 {
        let low_index = (low as f32 / FFT_RESOLUTION) as usize;
        let high_index = std::cmp::min((high as f32 / FFT_RESOLUTION) as usize, self.raw_bins.len() - 1);
        if low_index >= high_index {
            return 0.0;
        }
        let data = &self.raw_bins[low_index..=high_index];
        data.iter().sum::<f32>() / (high_index - low_index) as f32
    }

    // Computes the frequency amplitude using interpolation between 2 closest bins
    fn get_frequency_amplitude(&self, frequency: &usize) -> Option<f32> {
        let precise_index =
            frequency.to_f32().unwrap_or(0.0) / FFT_RESOLUTION.to_f32().unwrap_or(1.0);
        let min_index = precise_index.floor().to_usize()?;
        let max_index = precise_index.ceil().to_usize()?;
        let position_between_bins = (frequency - self.get_bin_frequency_at_index(&min_index))
            .to_f32()
            .unwrap_or(0.0)
            / FFT_RESOLUTION.to_f32().unwrap_or(1.0);
        let amplitude = self.raw_bins.get(min_index)? * position_between_bins
            + self.raw_bins.get(max_index)? * (1.0 - position_between_bins);
        Some(amplitude)
    }

    fn get_bin_frequency_at_index(&self, index: &usize) -> usize {
        (*index as f32 * FFT_RESOLUTION) as usize
    }
}

pub struct AudioSignalProcessor {
    audio_sample_buffer: dasp_ring_buffer::Fixed<[f32; FFT_SIZE]>,
    audio_sample_rx: ringbuf::HeapConsumer<f32>,
    tmp_vec: Vec<f32>,
    fft_plan: Arc<dyn rustfft::Fft<f32>>,
    fft_compute_buffer: Vec<Complex<f32>>,
    fft_window_buffer: Vec<Complex<f32>>,
    pub fft_result: Arc<RwLock<FftResult>>,
}

impl AudioSignalProcessor {
    pub fn new(audio_rx: ringbuf::HeapConsumer<f32>) -> Self {
        let mut planner = rustfft::FftPlanner::new();
        Self {
            audio_sample_buffer: dasp_ring_buffer::Fixed::from([0f32; FFT_SIZE]),
            audio_sample_rx: audio_rx,
            tmp_vec: vec![0f32; FFT_SIZE],
            fft_compute_buffer: vec![Complex::<f32>::default(); FFT_SIZE],
            fft_plan: planner.plan_fft_forward(FFT_SIZE),
            fft_window_buffer: vec![],
            fft_result: Arc::default(),
        }
    }

    pub fn compute_fft(&mut self) {
        let sample_count = self.audio_sample_rx.pop_slice(self.tmp_vec.as_mut_slice());
        self.tmp_vec.iter().take(sample_count).for_each(|sample| {
            self.audio_sample_buffer.push(*sample);
        });

        self.fft_window_buffer = dasp_signal::from_iter(
            self.audio_sample_buffer
                .iter()
                .map(|e| e.to_sample::<f32>()),
        )
        .scale_amp(1.0)
        .take(FFT_SIZE)
        .enumerate()
        .map(|(index, value)| {
            let hann_factor = dasp_window::Hanning::window(index as f32 / (FFT_SIZE as f32 - 1.0));
            Complex::<f32> {
                re: value * hann_factor,
                im: 0.0,
            }
        })
        .collect();

        self.fft_plan.process_with_scratch(
            &mut self.fft_window_buffer,
            &mut self.fft_compute_buffer,
        );


        let mut fft_result_writeable = self.fft_result.write().unwrap();
        fft_result_writeable.raw_bins = self
            .fft_window_buffer
            .iter()
            .map(|bin| bin.norm_sqr() / FFT_SIZE.to_f32().unwrap_or(1.0).sqrt())
            .collect();
    }
}
