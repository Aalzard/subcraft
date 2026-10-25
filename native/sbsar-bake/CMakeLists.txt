cmake_minimum_required(VERSION 3.16)
project(subcraft-sbsar-bake CXX)
set(CMAKE_CXX_STANDARD 11)

if(NOT SUBSTANCE_SDK_ROOT)
    if(DEFINED ENV{SUBSTANCE_SDK_ROOT})
        set(SUBSTANCE_SDK_ROOT "$ENV{SUBSTANCE_SDK_ROOT}")
    else()
        set(SUBSTANCE_SDK_ROOT "${CMAKE_CURRENT_LIST_DIR}/../../_sdk_extract/substance-win-x64-v9.6.2-bf39d392")
    endif()
endif()

set(BUILD_FRAMEWORK_TUTORIALS OFF CACHE BOOL "" FORCE)
add_subdirectory("${SUBSTANCE_SDK_ROOT}" substance_sdk EXCLUDE_FROM_ALL)

add_executable(subcraft-sbsar-bake
    main.cpp
    texture_export.cpp
)

target_include_directories(subcraft-sbsar-bake PRIVATE
    "${SUBSTANCE_SDK_ROOT}/include"
)

target_compile_definitions(subcraft-sbsar-bake PRIVATE SUBSTANCE_PLATFORM_BLEND _CRT_SECURE_NO_WARNINGS)
target_link_libraries(subcraft-sbsar-bake PRIVATE substance_framework tinyxml)

if(WIN32)
    set(PREBUILT_LIB_PATH_DEBUG "${SUBSTANCE_SDK_ROOT}/lib/debug")
    set(PREBUILT_LIB_PATH_RELEASE "${SUBSTANCE_SDK_ROOT}/lib/release")
    find_library(DBG_LIB_substance_linker substance_linker PATHS ${PREBUILT_LIB_PATH_DEBUG} NO_DEFAULT_PATH)
    find_library(REL_LIB_substance_linker substance_linker PATHS ${PREBUILT_LIB_PATH_RELEASE} NO_DEFAULT_PATH)
    find_library(DBG_LIB_substance_sse2_blend substance_sse2_blend PATHS ${PREBUILT_LIB_PATH_DEBUG} NO_DEFAULT_PATH)
    find_library(REL_LIB_substance_sse2_blend substance_sse2_blend PATHS ${PREBUILT_LIB_PATH_RELEASE} NO_DEFAULT_PATH)
    target_link_libraries(subcraft-sbsar-bake PRIVATE
        debug ${DBG_LIB_substance_linker} optimized ${REL_LIB_substance_linker}
        debug ${DBG_LIB_substance_sse2_blend} optimized ${REL_LIB_substance_sse2_blend}
    )
endif()

set_target_properties(subcraft-sbsar-bake PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_CURRENT_LIST_DIR}/../../bin/sbsar-bake"
    RUNTIME_OUTPUT_DIRECTORY_DEBUG "${CMAKE_CURRENT_LIST_DIR}/../../bin/sbsar-bake"
    RUNTIME_OUTPUT_DIRECTORY_RELEASE "${CMAKE_CURRENT_LIST_DIR}/../../bin/sbsar-bake"
)
