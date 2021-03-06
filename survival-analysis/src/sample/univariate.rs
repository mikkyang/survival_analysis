use super::{
    IntervalCensored, LeftCensored, LeftTruncation, LogLikelihood, PartiallyObserved,
    RightCensored, Uncensored, Weighted,
};
use crate::distribution::{CumulativeHazard, LogCumulativeDensity, LogHazard, Survival};
use ndarray::prelude::*;
use ndarray::{Data, OwnedRepr, RawData, ScalarOperand};
use num_traits::{clamp, Float, FromPrimitive};
use std::ops::{Add, Neg, Sub};

/// A convenience method to convert a list of events and their observed status
/// into a set of partially observed events.
pub trait FromEvents<'a, F: 'a> {
    fn from_events(
        events: impl IntoIterator<Item = &'a F>,
        event_observed: impl IntoIterator<Item = &'a bool>,
    ) -> Self;
}

impl<'a, F, C> FromEvents<'a, F> for PartiallyObserved<OwnedRepr<F>, Ix1, C>
where
    F: 'a + Copy,
    C: From<Vec<F>>,
{
    fn from_events(
        events: impl IntoIterator<Item = &'a F>,
        event_observed: impl IntoIterator<Item = &'a bool>,
    ) -> Self {
        let mut observed_events = Vec::new();
        let mut censored_events = Vec::new();

        for (&event, &o) in events.into_iter().zip(event_observed) {
            if o {
                observed_events.push(event)
            } else {
                censored_events.push(event)
            }
        }

        PartiallyObserved {
            observed: Uncensored(Array::from(observed_events)),
            censored: C::from(censored_events),
        }
    }
}

impl<'a, F, C> FromEvents<'a, (F, F)> for PartiallyObserved<OwnedRepr<F>, Ix1, C>
where
    F: 'a + Copy,
    C: From<(Vec<F>, Vec<F>)>,
{
    fn from_events(
        events: impl IntoIterator<Item = &'a (F, F)>,
        event_observed: impl IntoIterator<Item = &'a bool>,
    ) -> Self {
        let mut observed_events = Vec::new();
        let mut censored_starts = Vec::new();
        let mut censored_stops = Vec::new();

        for (&event, &o) in events.into_iter().zip(event_observed) {
            if o {
                let (_, time) = event;
                observed_events.push(time)
            } else {
                let (start, stop) = event;
                censored_starts.push(start);
                censored_stops.push(stop);
            }
        }

        PartiallyObserved {
            observed: Uncensored(Array::from(observed_events)),
            censored: C::from((censored_starts, censored_stops)),
        }
    }
}

impl<T, F> LeftTruncation<T, Ix1>
where
    T: Data<Elem = F>,
    F: Float,
{
    pub fn new(entry_time: ArrayBase<T, Ix1>) -> Result<Self, ()> {
        let zero = F::zero();
        if entry_time.iter().find(|&&t| t <= zero).is_some() {
            Err(())
        } else {
            Ok(LeftTruncation(entry_time))
        }
    }
}

impl<F> From<Vec<F>> for RightCensored<OwnedRepr<F>, Ix1> {
    fn from(vec: Vec<F>) -> Self {
        RightCensored(Array::from(vec))
    }
}

impl<F> From<Vec<F>> for LeftCensored<OwnedRepr<F>, Ix1> {
    fn from(vec: Vec<F>) -> Self {
        LeftCensored(Array::from(vec))
    }
}

impl<F> From<(Vec<F>, Vec<F>)> for IntervalCensored<OwnedRepr<F>, Ix1> {
    fn from((start, stop): (Vec<F>, Vec<F>)) -> Self {
        IntervalCensored {
            start: Array::from(start),
            stop: Array::from(stop),
        }
    }
}

impl<D, F, T, W> LogLikelihood<D, F> for Weighted<T, W, Ix1>
where
    T: LogLikelihood<D, Array1<F>>,
    F: Float + ScalarOperand,
    W: Data<Elem = F>,
{
    fn log_likelihood(&self, distribution: &D) -> F {
        let Weighted { time, weight } = self;

        let log_likelihood = time.log_likelihood(distribution);
        (weight * &log_likelihood).sum() / weight.sum()
    }
}

impl<D, O, T, C> LogLikelihood<D, O> for PartiallyObserved<T, Ix1, C>
where
    D: LogHazard<ArrayBase<T, Ix1>, O> + CumulativeHazard<ArrayBase<T, Ix1>, O>,
    O: Sub<Output = O> + Add<Output = O>,
    T: RawData,
    C: LogLikelihood<D, O>,
{
    fn log_likelihood(&self, distribution: &D) -> O {
        let PartiallyObserved { observed, censored } = self;
        observed.log_likelihood(distribution) + censored.log_likelihood(distribution)
    }
}

impl<D, O, T> LogLikelihood<D, O> for Uncensored<T, Ix1>
where
    D: LogHazard<ArrayBase<T, Ix1>, O> + CumulativeHazard<ArrayBase<T, Ix1>, O>,
    O: Sub<Output = O>,
    T: RawData,
{
    fn log_likelihood(&self, distribution: &D) -> O {
        let Uncensored(time) = self;
        distribution.log_hazard(time) - distribution.cumulative_hazard(time)
    }
}

impl<D, O, T> LogLikelihood<D, O> for RightCensored<T, Ix1>
where
    D: LogHazard<ArrayBase<T, Ix1>, O> + CumulativeHazard<ArrayBase<T, Ix1>, O>,
    O: Neg<Output = O>,
    T: RawData,
{
    fn log_likelihood(&self, distribution: &D) -> O {
        let RightCensored(time) = self;
        -distribution.cumulative_hazard(&time)
    }
}

impl<D, O, T> LogLikelihood<D, O> for LeftCensored<T, Ix1>
where
    D: LogCumulativeDensity<ArrayBase<T, Ix1>, O>,
    T: RawData,
{
    fn log_likelihood(&self, distribution: &D) -> O {
        let LeftCensored(time) = self;
        distribution.log_cumulative_density(&time)
    }
}

/// A trait used to allow both log likelihood to return both scalar and vector
/// types for IntervalCensored data.
pub trait UpstreamTraitHack {}

impl UpstreamTraitHack for f32 {}
impl UpstreamTraitHack for f64 {}

impl<D, F, T> LogLikelihood<D, F> for IntervalCensored<T, Ix1>
where
    D: Survival<ArrayBase<T, Ix1>, Array1<F>>,
    F: Float + FromPrimitive + UpstreamTraitHack,
    T: Data<Elem = F>,
{
    fn log_likelihood(&self, distribution: &D) -> F {
        let array: Array1<F> = self.log_likelihood(distribution);
        array.sum()
    }
}

impl<D, F, T> LogLikelihood<D, Array1<F>> for IntervalCensored<T, Ix1>
where
    D: Survival<ArrayBase<T, Ix1>, Array1<F>>,
    F: Float + FromPrimitive,
    T: Data<Elem = F>,
{
    fn log_likelihood(&self, distribution: &D) -> Array1<F> {
        let IntervalCensored { start, stop } = self;

        let min = F::from_f64(-1e50).unwrap();
        let max = F::from_f64(1e50).unwrap();

        let survival = (distribution.survival(&start) - distribution.survival(&stop))
            .mapv_into(F::ln)
            .mapv_into(|x| clamp(x, min, max));

        survival
    }
}

impl<D, F, T> LogLikelihood<D, Array1<F>> for LeftTruncation<T, Ix1>
where
    D: CumulativeHazard<ArrayBase<T, Ix1>, Array1<F>>,
    F: Float + FromPrimitive,
    T: Data<Elem = F>,
{
    fn log_likelihood(&self, distribution: &D) -> Array1<F> {
        let LeftTruncation(entry_time) = self;
        distribution.cumulative_hazard(entry_time)
    }
}
