#import <Foundation/Foundation.h>
#import <Sparkle/Sparkle.h>

static SPUStandardUpdaterController *controller = nil;

extern "C" void sew_updater_start(void) {
    if (controller == nil) {
        controller = [[SPUStandardUpdaterController alloc]
            initWithStartingUpdater:YES
            updaterDelegate:nil
            userDriverDelegate:nil];
    }
}

extern "C" void sew_check_for_updates(void) {
    sew_updater_start();
    [controller checkForUpdates:nil];
}
