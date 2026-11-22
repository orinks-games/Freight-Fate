// The app's sound session and its time in the background.
//
// The rules for leaving and coming back live in Rust
// (`ff_core::app_lifecycle`, applied in `src/app/lifecycle.rs`). SDL already
// reports the four app notifications (resign active, background, foreground,
// become active); this file adds what SDL does not see and the two things
// only UIKit can do:
//
// - The sound session's category is Playback, set before the audio engine
//   opens its device, so the game plays with the ring/silent switch on
//   silent. cpal 0.18 sets no category (iOS's default, SoloAmbient, follows
//   the switch) but does stop and resume its own stream on an interruption.
// - An interruption (a call, an alarm, Siri) and a lost output (headphones
//   out, a Bluetooth device gone) are queued for the game loop as event
//   codes, in the order of `AppEvent::ALL`.
// - Entering the background asks iOS for extra time, which the game ends
//   once its background save is written (`ff_lifecycle_background_saved`).
// - The app being ended from the foreground marks the problem reports run as
//   ended, synchronously, because nothing runs after that notification.

#import <AVFoundation/AVFoundation.h>
#import <UIKit/UIKit.h>
#import <os/lock.h>

// Codes, matching `AppEvent::ALL`'s order.
enum {
    FF_AUDIO_INTERRUPTION_BEGAN = 4,
    FF_AUDIO_INTERRUPTION_ENDED = 5,
    FF_OUTPUT_DEVICE_LOST = 6,
};

#define FF_LIFECYCLE_CAPACITY 32

static int32_t ff_lifecycle_queue[FF_LIFECYCLE_CAPACITY];
static unsigned ff_lifecycle_head = 0;
static unsigned ff_lifecycle_len = 0;
static os_unfair_lock ff_lifecycle_lock = OS_UNFAIR_LOCK_INIT;
static UIBackgroundTaskIdentifier ff_background_task = UIBackgroundTaskInvalid;

static void ff_lifecycle_push(int32_t code) {
    os_unfair_lock_lock(&ff_lifecycle_lock);
    if (ff_lifecycle_len < FF_LIFECYCLE_CAPACITY) {
        unsigned tail = (ff_lifecycle_head + ff_lifecycle_len) % FF_LIFECYCLE_CAPACITY;
        ff_lifecycle_queue[tail] = code;
        ff_lifecycle_len++;
    }
    os_unfair_lock_unlock(&ff_lifecycle_lock);
}

int32_t ff_lifecycle_next(void) {
    int32_t code = -1;
    os_unfair_lock_lock(&ff_lifecycle_lock);
    if (ff_lifecycle_len > 0) {
        code = ff_lifecycle_queue[ff_lifecycle_head];
        ff_lifecycle_head = (ff_lifecycle_head + 1) % FF_LIFECYCLE_CAPACITY;
        ff_lifecycle_len--;
    }
    os_unfair_lock_unlock(&ff_lifecycle_lock);
    return code;
}

static void ff_end_background_task(void) {
    if (ff_background_task != UIBackgroundTaskInvalid) {
        [UIApplication.sharedApplication endBackgroundTask:ff_background_task];
        ff_background_task = UIBackgroundTaskInvalid;
    }
}

void ff_lifecycle_background_saved(void) {
    if (NSThread.isMainThread) {
        ff_end_background_task();
    } else {
        dispatch_async(dispatch_get_main_queue(), ^{ ff_end_background_task(); });
    }
}

int32_t ff_lifecycle_wake_audio(void) {
    NSError *error = nil;
    if (![AVAudioSession.sharedInstance setActive:YES error:&error]) {
        NSLog(@"Freight Fate: the sound session would not start again: %@", error);
        return 0;
    }
    return 1;
}

void ff_lifecycle_install(void (*terminating)(void)) {
    AVAudioSession *session = AVAudioSession.sharedInstance;
    NSError *error = nil;
    if (![session setCategory:AVAudioSessionCategoryPlayback error:&error]) {
        NSLog(@"Freight Fate: the sound session category was refused: %@", error);
    }
    error = nil;
    if (![session setActive:YES error:&error]) {
        NSLog(@"Freight Fate: the sound session would not start: %@", error);
    }

    NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
    NSOperationQueue *main = NSOperationQueue.mainQueue;
    [center addObserverForName:AVAudioSessionInterruptionNotification
                        object:session
                         queue:main
                    usingBlock:^(NSNotification *note) {
                        NSNumber *type = note.userInfo[AVAudioSessionInterruptionTypeKey];
                        if (type.unsignedIntegerValue == AVAudioSessionInterruptionTypeBegan) {
                            ff_lifecycle_push(FF_AUDIO_INTERRUPTION_BEGAN);
                        } else {
                            ff_lifecycle_push(FF_AUDIO_INTERRUPTION_ENDED);
                        }
                    }];
    [center addObserverForName:AVAudioSessionRouteChangeNotification
                        object:session
                         queue:main
                    usingBlock:^(NSNotification *note) {
                        NSNumber *reason = note.userInfo[AVAudioSessionRouteChangeReasonKey];
                        if (reason.unsignedIntegerValue ==
                            AVAudioSessionRouteChangeReasonOldDeviceUnavailable) {
                            ff_lifecycle_push(FF_OUTPUT_DEVICE_LOST);
                        }
                    }];
    [center addObserverForName:UIApplicationDidEnterBackgroundNotification
                        object:nil
                         queue:main
                    usingBlock:^(__unused NSNotification *note) {
                        if (ff_background_task != UIBackgroundTaskInvalid) {
                            return;
                        }
                        ff_background_task = [UIApplication.sharedApplication
                            beginBackgroundTaskWithName:@"Save the drive"
                                      expirationHandler:^{ ff_end_background_task(); }];
                    }];
    [center addObserverForName:UIApplicationWillTerminateNotification
                        object:nil
                         queue:nil
                    usingBlock:^(__unused NSNotification *note) { terminating(); }];
}
