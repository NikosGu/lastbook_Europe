# Included by llama.cpp's CMake right after each project() call: llama-cpp-sys-2 passes CMAKE_* variables on from the
# environment, and .cargo/config.toml sets CMAKE_PROJECT_INCLUDE to this file. Two changes to how llama.cpp is built for
# Lastbook's local AI runner; change GGML_LASTBOOK_BUILD in .cargo/config.toml with this file, so llama.cpp is rebuilt.
#
# 1. No model downloader. The runner never downloads anything, and must not be able to. llama.cpp's common library carries
#    an HTTP client (cpp-httplib) for fetching models; linked statically, as on a Mac, the runner takes only what it uses
#    and the client is left out, but with Vulkan's loadable backends on Windows and Linux every llama.cpp library is shared
#    and the whole client would ship. So the library's two downloading files are replaced by no-download.cpp, which
#    answers that nothing can be fetched, and the HTTP client is not linked. lastbook-tools scan-package checks the result.
#
# 2. A graphics processor's weights copied, not mapped, when asked (LASTBOOK_COPY_GPU_WEIGHTS in the environment). With
#    the file mapped, llama.cpp hands a graphics device the mapped range from the first to the last of its tensors. When a
#    mixture of experts runs from the disk, the experts stay on the CPU and lie between the graphics processor's tensors
#    layer by layer, so on a Mac that range is nearly the whole file, which Metal must then hold in memory. Copied, the
#    graphics processor holds only its own weights. src/llama-model.cpp is rebuilt with that one condition added; if
#    llama.cpp's line has changed, the build stops here rather than build something else.

get_property(lastbook_done GLOBAL PROPERTY LASTBOOK_LLAMA_BUILD)
if (NOT lastbook_done)   # once: overriding a command twice would make it call itself
    set_property(GLOBAL PROPERTY LASTBOOK_LLAMA_BUILD "${CMAKE_CURRENT_LIST_DIR}")

    function(lastbook_patched_model_source out)
        set(src "${CMAKE_CURRENT_SOURCE_DIR}/llama-model.cpp")
        file(READ "${src}" text)
        set(line "        if ((ml.use_mmap || is_lazy_mapped) && use_mmap_buffer && buffer_from_host_ptr_supported && is_default_buft) {")
        string(FIND "${text}" "${line}" at)
        if (at EQUAL -1)
            message(FATAL_ERROR "lastbook.cmake: llama-model.cpp no longer has the line it changes; look at this file again")
        endif()
        set(new "        // Lastbook (crates/llm/llama/lastbook.cmake): a graphics device's weights copied, not mapped, when asked\n")
        string(APPEND new "        const bool lastbook_copy = std::getenv(\"LASTBOOK_COPY_GPU_WEIGHTS\") != nullptr && ggml_backend_dev_type(dev) != GGML_BACKEND_DEVICE_TYPE_CPU;\n")
        string(APPEND new "        if ((ml.use_mmap || is_lazy_mapped) && use_mmap_buffer && buffer_from_host_ptr_supported && is_default_buft && !lastbook_copy) {")
        string(REPLACE "${line}" "${new}" text "${text}")
        set(dst "${CMAKE_CURRENT_BINARY_DIR}/lastbook/llama-model.cpp")
        file(WRITE "${dst}.new" "#include <cstdlib>\n${text}")
        file(COPY_FILE "${dst}.new" "${dst}" ONLY_IF_DIFFERENT)
        set(${out} "${dst}" PARENT_SCOPE)
    endfunction()

    function(add_library name)
        get_property(here GLOBAL PROPERTY LASTBOOK_LLAMA_BUILD)
        set(sources ${ARGN})
        if (name STREQUAL "llama-common")
            list(FILTER sources EXCLUDE REGEX "(^|/)(download|hf-cache)\\.cpp$")
            list(APPEND sources "${here}/no-download.cpp")
        elseif (name STREQUAL "llama")
            lastbook_patched_model_source(patched)
            list(TRANSFORM sources REPLACE "^llama-model\\.cpp$" "${patched}")
            # as the original: compiled on its own, not in a unity batch with the model files
            set_source_files_properties("${patched}" PROPERTIES SKIP_UNITY_BUILD_INCLUSION ON)
        endif()
        _add_library(${name} ${sources})
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
