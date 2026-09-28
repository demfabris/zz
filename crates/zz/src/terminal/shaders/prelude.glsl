#version 450 core

layout(set = 0, binding = 1, std140) uniform Globals {
    float iTime;
    float iTimeDelta;
    float iFrameRate;
    int   iFrame;
    vec4  iChannelTime;
    vec3  iChannelResolution[4];
    vec4  iMouse;
    vec4  iDate;
    float iSampleRate;
    vec4  iCurrentCursor;
    vec4  iPreviousCursor;
    vec4  iCurrentCursorColor;
    vec4  iPreviousCursorColor;
    int   iCurrentCursorStyle;
    int   iPreviousCursorStyle;
    int   iCursorVisible;
    float iTimeCursorChange;
    float iTimeFocus;
    int   iFocus;
    vec3  iPalette[256];
    vec3  iBackgroundColor;
    vec3  iForegroundColor;
    vec3  iCursorColor;
    vec3  iCursorText;
    vec3  iSelectionForegroundColor;
    vec3  iSelectionBackgroundColor;
    vec2  iCellSize;
    float iTimeCopy;
    int   iCopyRectCount;
    vec4  iCopyRects[16];
};

#define CURSORSTYLE_BLOCK        0
#define CURSORSTYLE_BLOCK_HOLLOW 1
#define CURSORSTYLE_BAR          2
#define CURSORSTYLE_UNDERLINE    3
#define CURSORSTYLE_LOCK         4

layout(set = 0, binding = 0) uniform texture2D zz_Channel0;
layout(set = 0, binding = 2) uniform sampler zz_Sampler0;
#define iChannel0 sampler2D(zz_Channel0, zz_Sampler0)
#define iResolution vec3(vec2(textureSize(iChannel0, 0)), 1.0)
#define texture2D texture

layout(location = 0) in vec2 zz_FragCoord;
layout(location = 0) out vec4 zz_FragColor;
