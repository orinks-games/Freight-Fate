# The toolchain sdl2-sys builds the bundled SDL2 with for Android, passed in
# through its SDL2_TOOLCHAIN variable (`cargo tools build-android` sets it).
#
# It is the NDK's own toolchain plus one header forced into SDL's C files
# by sdl2-android-project.cmake. SDL 2.26's sensor module, which tilt
# steering reads the accelerometer through, calls ALooper_pollAll, and NDK
# r27 and later refuse to compile it; ff_looper_poll_all.h stands in for it
# with ALooper_pollOnce, rather than pinning an older NDK.
# `cargo tools build-android` rebuilds SDL when a file in this folder
# changes.
#
# ANDROID_ABI and ANDROID_PLATFORM come from the environment so one file
# serves every ABI; the NDK's own defaults (armeabi-v7a, its lowest API)
# would otherwise apply, because sdl2-sys passes neither.
if(DEFINED ENV{FF_ANDROID_ABI})
  set(ANDROID_ABI "$ENV{FF_ANDROID_ABI}")
endif()
if(DEFINED ENV{FF_ANDROID_PLATFORM})
  set(ANDROID_PLATFORM "$ENV{FF_ANDROID_PLATFORM}")
endif()
file(TO_CMAKE_PATH "$ENV{ANDROID_NDK_HOME}" FF_NDK)
include("${FF_NDK}/build/cmake/android.toolchain.cmake")
set(CMAKE_PROJECT_INCLUDE "${CMAKE_CURRENT_LIST_DIR}/sdl2-android-project.cmake")
