/*
 * Forced into every C file of the bundled SDL2 by
 * sdl2-android-project.cmake.
 *
 * SDL 2.26's Android sensor module (src/sensor/android/SDL_androidsensor.c)
 * reads the accelerometer with ALooper_pollAll, which NDK r27 and later
 * declare removed, so the file does not compile. <android/looper.h>
 * describes ALooper_pollAll as ALooper_pollOnce repeated while it only ran
 * callbacks, and says to call ALooper_pollOnce instead; this stands in for
 * it with exactly that. SDL's one call passes a timeout of 0.
 */
#ifndef FF_LOOPER_POLL_ALL_H
#define FF_LOOPER_POLL_ALL_H

#if defined(__ANDROID__) && !defined(__cplusplus)
#include <android/looper.h>

static inline int ff_looper_poll_all(int timeout_millis, int *out_fd, int *out_events,
                                     void **out_data)
{
    int result;
    do {
        result = ALooper_pollOnce(timeout_millis, out_fd, out_events, out_data);
    } while (result == ALOOPER_POLL_CALLBACK);
    return result;
}

#define ALooper_pollAll ff_looper_poll_all
#endif

#endif
