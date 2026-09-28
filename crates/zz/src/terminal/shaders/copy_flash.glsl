float roundedBoxDistance(vec2 point, vec4 rect, float radius) {
    vec2 center = 0.5 * (rect.xy + rect.zw);
    vec2 halfSize = 0.5 * (rect.zw - rect.xy);
    vec2 q = abs(point - center) - halfSize + radius;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
}

float smoothUnion(float a, float b, float k) {
    float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

float easeInOut(float t) {
    return 0.5 - 0.5 * cos(3.14159265 * t);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 texel = 1.0 / iResolution.xy;
    vec2 uv = fragCoord * texel;
    vec4 color = texture(iChannel0, uv);
    float progress = clamp((iTime - iTimeCopy) / FLASH_SECONDS, 0.0, 1.0);
    float cell = max(iCellSize.y, 1.0);
    float slant = 0.6;

    float distance = 1.0e9;
    float sweepStart = 1.0e9;
    float sweepEnd = -1.0e9;
    for (int i = 0; i < 16; i++) {
        if (i >= iCopyRectCount) {
            break;
        }
        vec4 rect = iCopyRects[i];
        distance = smoothUnion(distance, roundedBoxDistance(fragCoord, rect, 0.2 * cell), 0.25 * cell);
        sweepStart = min(sweepStart, rect.x + slant * rect.y);
        sweepEnd = max(sweepEnd, rect.z + slant * rect.w);
    }

    float width = 1.6 * cell;
    float center = mix(sweepStart - 1.5 * width, sweepEnd + 1.5 * width, easeInOut(progress));
    float offset = (fragCoord.x + slant * fragCoord.y - center) / width;
    float band = exp(-offset * offset);

    float inside = 1.0 - smoothstep(-0.75, 0.75, distance);
    float edge = exp(-max(distance, 0.0) / (0.3 * cell)) * (1.0 - inside);

    vec2 reach = vec2(0.6 * iCellSize.x, 0.5 * cell) * texel;
    vec3 around = 0.25 * (texture(iChannel0, uv + vec2(reach.x, 0.0)).rgb
        + texture(iChannel0, uv - vec2(reach.x, 0.0)).rgb
        + texture(iChannel0, uv + vec2(0.0, reach.y)).rgb
        + texture(iChannel0, uv - vec2(0.0, reach.y)).rgb);
    vec3 selection = iSelectionBackgroundColor * color.a;
    vec3 backdrop = iBackgroundColor * color.a;
    vec3 base = distance(around, selection) < distance(around, backdrop) ? selection : backdrop;
    float glyph = smoothstep(0.04, 0.2, distance(color.rgb, base));
    vec3 detail = (color.rgb - base) * 0.8 * glyph * band * inside;

    vec3 light = mix(iSelectionBackgroundColor, vec3(1.0), 0.6);
    vec3 sheen = light * band * (0.22 * inside + 0.2 * edge);
    vec3 shimmered = clamp(color.rgb + detail + sheen, 0.0, 1.0);
    float coverage = max(sheen.r, max(sheen.g, sheen.b));
    fragColor = vec4(shimmered, min(1.0, color.a + coverage));
}
