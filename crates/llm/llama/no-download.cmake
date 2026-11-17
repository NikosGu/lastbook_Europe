# Included by llama.cpp's CMake right after each project() call: llama-cpp-sys-2 passes CMAKE_* variables on from the
# environment, and .cargo/config.toml sets CMAKE_PROJECT_INCLUDE to this file.
#
# Lastbook's local AI runner never downloads anything, and must not be able to. llama.cpp's common library carries an
# HTTP client (cpp-httplib) for fetching models; linked statically, as on a Mac, the runner takes only what it uses and
# the client is left out, but with Vulkan's loadable backends on Windows and Linux every llama.cpp library is shared and
# the whole client would ship. So the library's two downloading files are replaced by no-download.cpp, which answers
# that nothing can be fetched, and the HTTP client is not linked. lastbook-tools scan-package checks the result.

get_property(lastbook_done GLOBAL PROPERTY LASTBOOK_NO_DOWNLOAD)
if (NOT lastbook_done)   # once: overriding a command twice would make it call itself
    set_property(GLOBAL PROPERTY LASTBOOK_NO_DOWNLOAD "${CMAKE_CURRENT_LIST_DIR}/no-download.cpp")

    function(add_library name)
        if (name STREQUAL "llama-common")
            get_property(stub GLOBAL PROPERTY LASTBOOK_NO_DOWNLOAD)
            set(sources ${ARGN})
            list(FILTER sources EXCLUDE REGEX "(^|/)(download|hf-cache)\\.cpp$")
            _add_library(${name} ${sources} ${stub})
        else()
            _add_library(${name} ${ARGN})
        endif()
    endfunction()

    function(target_link_libraries target)
        set(args ${ARGN})
        if (target STREQUAL "llama-common")
            list(REMOVE_ITEM args cpp-httplib)
            list(LENGTH args n)
            if (n LESS_EQUAL 1)   # only a keyword was left, such as PRIVATE
                return()
            endif()
        endif()
        _target_link_libraries(${target} ${args})
    endfunction()
endif()
