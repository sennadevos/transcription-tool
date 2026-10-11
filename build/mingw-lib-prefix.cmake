# ggml drops the "lib" prefix on all WIN32 builds, but rustc's windows-gnu target looks for libggml.a.
# Injected via CMAKE_PROJECT_INCLUDE; restores the prefix once all targets exist.
function(_restore_lib_prefix)
    foreach(t ggml ggml-base ggml-cpu)
        if(TARGET ${t})
            set_target_properties(${t} PROPERTIES PREFIX "lib")
        endif()
    endforeach()
endfunction()
get_property(_deferred GLOBAL PROPERTY _restore_lib_prefix_deferred)
if(NOT _deferred)
    set_property(GLOBAL PROPERTY _restore_lib_prefix_deferred TRUE)
    cmake_language(DEFER DIRECTORY ${CMAKE_SOURCE_DIR} CALL _restore_lib_prefix)
endif()
