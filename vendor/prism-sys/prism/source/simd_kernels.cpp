// SPDX-License-Identifier: MPL-2.0

#include <algorithm>
#include <span>

#undef HWY_TARGET_INCLUDE
#define HWY_TARGET_INCLUDE "simd_kernels.cpp"
// clang-format off
#include "hwy/foreach_target.h"
#include "hwy/highway.h"
#include "hwy/contrib/dot/dot-inl.h"
#include "hwy/contrib/math/math-inl.h"
// clang-format on

HWY_BEFORE_NAMESPACE();
namespace HWY_NAMESPACE {
namespace hn = hwy::HWY_NAMESPACE;

HWY_ATTR void fill_frame_db_impl(std::span<const float> interleaved,
                                 std::size_t channels, std::size_t frame_len,
                                 std::size_t hop, std::size_t total_frames,
                                 std::span<float> db) {
  const hn::ScalableTag<float> d;
  const std::size_t n = db.size();
  for (std::size_t i = 0; i < n; ++i) {
    const auto start_frame = i * hop;
    const std::size_t end_frame =
        std::min(start_frame + frame_len, total_frames);
    if (end_frame <= start_frame) {
      db[i] = 0.0F;
      continue;
    }
    const auto nsamples = (end_frame - start_frame) * channels;
    const float *HWY_RESTRICT p = interleaved.data() + (start_frame * channels);
    db[i] =
        hn::Dot::Compute<0>(d, p, p, nsamples) / static_cast<float>(nsamples);
  }
  const auto N = hn::Lanes(d);
  const auto eps = hn::Set(d, 1e-16F);
  const auto scale = hn::Set(d, 10.0F);
  std::size_t i = 0;
  for (; i + N <= n; i += N) {
    const auto ms = hn::Add(hn::LoadU(d, db.data() + i), eps);
    hn::StoreU(hn::Mul(scale, hn::Log10(d, ms)), d, db.data() + i);
  }
  if (i < n) {
    const auto rest = n - i;
    const auto ms = hn::Add(hn::LoadN(d, db.data() + i, rest), eps);
    hn::StoreN(hn::Mul(scale, hn::Log10(d, ms)), d, db.data() + i, rest);
  }
}
} // namespace HWY_NAMESPACE
HWY_AFTER_NAMESPACE();

#if HWY_ONCE
HWY_EXPORT(fill_frame_db_impl);

void fill_frame_db(std::span<const float> interleaved, std::size_t channels,
                   std::size_t frame_len, std::size_t hop,
                   std::size_t total_frames, std::span<float> db) {
  HWY_DYNAMIC_DISPATCH(fill_frame_db_impl)
  (interleaved, channels, frame_len, hop, total_frames, db);
}
#endif
