// Copyright Materialize, Inc. All rights reserved.
//
// Use of this software is governed by the Business Source License
// included in the LICENSE file.
//
// As of the Change Date specified in that file, in accordance with
// the Business Source License, use of this software will be governed
// by the Apache License, Version 2.0.

use prometheus::core::{Atomic, Collector, GenericCounter, GenericGauge, GenericGaugeVec};
use prometheus::proto::MetricFamily;
use prometheus::{HistogramOpts, Opts, Registry};

pub use prometheus::{HistogramVec, IntGauge, UIntCounter, UIntGauge};

/// Buckets that can capture data between one microsecond and 1 second.
pub const HISTOGRAM_BUCKETS: [f64; 17] = [
    0.000_016, 0.000_032, 0.000_064, 0.000_128, 0.000_256, 0.000_512, 0.001, 0.002, 0.004, 0.008,
    0.016, 0.032, 0.064, 0.128, 0.256, 0.512, 1.0,
];

#[macro_export]
macro_rules! metric {
    (
        name: $name:expr,
        help: $help:expr
        $(, const_labels: { $($cl_key:expr => $cl_value:expr ),* })?
        $(, var_labels: [ $($vl_name:expr),* ])?
        $(,)?
    ) => {{
        let mut const_labels = ::std::collections::HashMap::<String, String>::new();
        $(
            $(
                const_labels.insert($cl_key.into(), $cl_value.into());
            )*
        )?
        let mut var_labels = ::std::vec::Vec::<String>::new();
        $(
            $(
                var_labels.push($vl_name.into());
            )*
        )?
        ::prometheus::Opts::new($name, $help)
            .const_labels(const_labels)
            .variable_labels(var_labels)
    }}
}

#[derive(Debug, Clone)]
pub struct MetricsRegistry {
    inner: Registry,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        MetricsRegistry {
            inner: Registry::new(),
        }
    }

    pub fn register<M>(&self, opts: prometheus::Opts) -> M
    where
        M: MakeCollector,
    {
        let collector = M::make_collector(opts);
        self.inner.register(Box::new(collector.clone())).unwrap();
        collector
    }

    pub fn gather(&self) -> Vec<MetricFamily> {
        self.inner.gather()
    }
}

pub trait MakeCollector: Collector + Clone + 'static {
    fn make_collector(opts: Opts) -> Self;
}

impl<T> MakeCollector for GenericCounter<T>
where
    T: Atomic + 'static,
{
    fn make_collector(opts: Opts) -> Self {
        Self::with_opts(opts).expect("blah")
    }
}

impl<T> MakeCollector for GenericGauge<T>
where
    T: Atomic + 'static,
{
    fn make_collector(opts: Opts) -> Self {
        Self::with_opts(opts).expect("blah")
    }
}

impl<T> MakeCollector for GenericGaugeVec<T>
where
    T: Atomic + 'static,
{
    fn make_collector(opts: Opts) -> Self {
        let labels = opts.variable_labels.clone();
        let labels = &labels.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        Self::new(opts, labels).expect("blah")
    }
}

impl MakeCollector for HistogramVec {
    fn make_collector(opts: Opts) -> Self {
        let labels = opts.variable_labels.clone();
        let labels = &labels.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        Self::new(
            HistogramOpts {
                common_opts: opts,
                buckets: HISTOGRAM_BUCKETS.to_vec(),
            },
            labels,
        )
        .expect("blah")
    }
}
