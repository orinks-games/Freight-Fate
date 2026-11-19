# The toolchain sdl2-sys builds the bundled SDL2 with for Android, passed in
# through its SDL2_TOOLCHAIN variable (`cargo tools build-android` sets it).
#
# It is the NDK's own toolchain plus one switch: SDL 2.26's sensor module
# calls ALooper_pollAll, which NDK r27 and later refuse to compile. The game
# reads no motion sensors (the tilt joystick is off on every phone), so the
# module is left out rather than pinning an older NDK.
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
set(SDL_SENSOR OFF CACHE BOOL "No motion sensors: see the comment above" FORCE)
