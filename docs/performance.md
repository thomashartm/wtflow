# Performance acceptance

Run `cargo bench -p wtflow-extract --bench performance --locked`.
The benchmark creates its own temporary source/index corpora; it invokes no
external indexers and requires no Node, Java, or Python installation.

Measured with Rust 1.80.1 release optimization on Apple M4 Pro, macOS 26.2.
Criterion uses 10 samples, 1 second warmup and at least 3 seconds measurement.
Files are read from the local filesystem with a warm OS cache.

| Operation | Measured interval | Limit |
| --- | --- | --- |
| Load/parse and detect entrypoints across 1,500 TS files | 43.080–46.823 ms | <500 ms |
| Load a 50,024,920-byte SCIP index | 50.041–50.463 ms | <1,000 ms |
| Extract at depth 3 after loading that index | 28.931–29.641 µs | <300 ms |

The 50 MB index has 1,016 documents and 4,236,975 source bytes. It combines the
real TypeScript fixture index with more than 350,000 external call references;
it is not padded with a large documentation string. Extraction uses the real
barrel/interface-dispatch fixture. Its warm measurement includes call-table
cache reuse. Corpus creation and initial source parsing are outside the loaded
extraction measurement and outside the index-only loading measurement.

The first index-load measurement was 2.85–3.07 seconds. Investigation found that
freshness hashing ran once per occurrence. Hashing once per document reduced
that cost without changing output or weakening freshness validation. The index
load benchmark was rerun after this change; the unrelated entrypoint/extraction
measurements above are from the initial run.

These are local acceptance results, not a latency guarantee for arbitrary
hardware, source complexity, storage, or index distributions.
