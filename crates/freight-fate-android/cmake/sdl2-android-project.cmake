# Read by SDL2's CMakeLists.txt right after its project() call
# (CMAKE_PROJECT_INCLUDE, set in sdl2-android-toolchain.cmake). The cmake
# crate passes its own CMAKE_C_FLAGS, which replace a toolchain file's, so
# the header SDL's C files need is added to them here instead.
string(APPEND CMAKE_C_FLAGS " -include \"${CMAKE_CURRENT_LIST_DIR}/ff_looper_poll_all.h\"")
