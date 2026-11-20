#version 440
layout(location = 0) in vec2 vUv;
layout(location = 1) in vec4 vColor;
layout(location = 0) out vec4 fragColor;
layout(std140, binding = 0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
};
layout(binding = 1) uniform sampler2D atlas;
void main() {
    vec4 t = texture(atlas, vUv);
    // Vertex alpha 0 marks a colour glyph (emoji): its own premultiplied colours.
    fragColor = vColor.a < 0.5 ? t * qt_Opacity : vec4(vColor.rgb, 1.0) * t.a * qt_Opacity;
}
