// The scene-graph pieces the grid item draws with: the glyph material and its shader, the
// background quads, and the images from kitty's graphics protocol.
#include "gridview_p.h"

#include <QtGui/QRegion>
#include <QtGui/QWindow>

#include <QtCore/QPointer>
#include <QtCore/QProcess>
#include <QtCore/QTimer>
#include <QtGui/QFontMetricsF>
#include <QtGui/QGuiApplication>
#include <QtGui/QClipboard>
#include <QtGui/QInputMethodEvent>
#include <QtGui/QKeyEvent>
#include <QtGui/QGlyphRun>
#include <QtGui/QPainter>
#include <QtGui/QPainterPath>
#include <QtGui/QTextLayout>
#include <QtQml/qqml.h>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGGeometryNode>
#include <QtQuick/QSGMaterial>
#include <QtQuick/QSGMaterialShader>
#include <QtQuick/QSGSimpleTextureNode>
#include <QtQuick/QSGTexture>
#include <QtQuick/QSGVertexColorMaterial>
#include <algorithm>
#include <cmath>
#include <cstring>

#include "kyko/src/bridge.cxx.h"

class GlyphShader : public QSGMaterialShader {
public:
    GlyphShader() {
        setShaderFileName(VertexStage, QStringLiteral(":/kyko/shaders/glyph.vert.qsb"));
        setShaderFileName(FragmentStage, QStringLiteral(":/kyko/shaders/glyph.frag.qsb"));
    }
    bool updateUniformData(RenderState& state, QSGMaterial*, QSGMaterial*) override {
        QByteArray* buf = state.uniformData();
        bool changed = false;
        if (state.isMatrixDirty()) {
            const QMatrix4x4 m = state.combinedMatrix();
            std::memcpy(buf->data(), m.constData(), 64);
            changed = true;
        }
        if (state.isOpacityDirty()) {
            const float o = state.opacity();
            std::memcpy(buf->data() + 64, &o, 4);
            changed = true;
        }
        return changed;
    }
    void updateSampledImage(RenderState& state, int binding, QSGTexture** texture, QSGMaterial* m,
                            QSGMaterial*) override {
        if (binding != 1)
            return;
        QSGTexture* t = static_cast<GlyphMaterial*>(m)->texture;
        t->commitTextureOperations(state.rhi(), state.resourceUpdateBatch());
        *texture = t;
    }
};

QSGMaterialShader* GlyphMaterial::createShader(QSGRendererInterface::RenderMode) const { return new GlyphShader; }

const QSGGeometry::AttributeSet& glyphAttributes() {
    static QSGGeometry::Attribute a[] = {
        QSGGeometry::Attribute::createWithAttributeType(0, 2, QSGGeometry::FloatType, QSGGeometry::PositionAttribute),
        QSGGeometry::Attribute::createWithAttributeType(1, 2, QSGGeometry::FloatType, QSGGeometry::TexCoordAttribute),
        QSGGeometry::Attribute::createWithAttributeType(2, 4, QSGGeometry::UnsignedByteType, QSGGeometry::ColorAttribute),
    };
    static QSGGeometry::AttributeSet set = {3, sizeof(GlyphVertex), a};
    return set;
}

QSGGeometryNode* makeBgNode() {
    auto* n = new QSGGeometryNode;
    auto* g = new QSGGeometry(QSGGeometry::defaultAttributes_ColoredPoint2D(), 0);
    g->setDrawingMode(QSGGeometry::DrawTriangles);
    g->setVertexDataPattern(QSGGeometry::StreamPattern);
    n->setGeometry(g);
    n->setMaterial(new QSGVertexColorMaterial);
    n->setFlags(QSGNode::OwnsGeometry | QSGNode::OwnsMaterial);
    return n;
}

void fillBg(QSGGeometryNode* node, const FfiCell* cells, const uint8_t* gutter, int cols, int rows, qreal cw,
            qreal ch, qreal w, qreal h, int curX, int curY, quint32 curColor, uchar alpha) {
    const quint32 def = kyko_palette_bg();
    int quads = 1 + (curX >= 0 ? 1 : 0);
    for (int r = 0; r < rows; ++r)
        if (gutter[r])
            ++quads;
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells + size_t(r) * cols;
        for (int c = 0; c < cols;) {
            int e = c + 1;
            while (e < cols && row[e].bg == row[c].bg)
                ++e;
            if (row[c].bg != def)
                ++quads;
            c = e;
        }
    }
    QSGGeometry* g = node->geometry();
    g->allocate(quads * 6);
    auto* p = g->vertexDataAsColoredPoint2D();
    // The default background, see-through by `window.opacity_pct` (premultiplied, as the vertex
    // colour material expects); the cells a program coloured stay opaque.
    {
        const uchar r = uchar(((def >> 16) & 0xff) * alpha / 255), g = uchar(((def >> 8) & 0xff) * alpha / 255),
                    b = uchar((def & 0xff) * alpha / 255);
        const float fw = float(w), fh = float(h);
        p[0].set(0, 0, r, g, b, alpha);
        p[1].set(fw, 0, r, g, b, alpha);
        p[2].set(0, fh, r, g, b, alpha);
        p[3].set(fw, 0, r, g, b, alpha);
        p[4].set(fw, fh, r, g, b, alpha);
        p[5].set(0, fh, r, g, b, alpha);
        p += 6;
    }
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells + size_t(r) * cols;
        for (int c = 0; c < cols;) {
            int e = c + 1;
            while (e < cols && row[e].bg == row[c].bg)
                ++e;
            if (row[c].bg != def)
                quad(p, float(c * cw), float(r * ch), float(e * cw), float((r + 1) * ch), row[c].bg);
            c = e;
        }
    }
    for (int r = 0; r < rows; ++r)
        if (gutter[r])
            quad(p, 0, float(r * ch), GUTTER_W, float((r + 1) * ch), kyko_palette_gutter(gutter[r]));
    if (curX >= 0) {
        const bool wide = cells[size_t(curY) * cols + curX].flags & WIDE;
        quad(p, float(curX * cw), float(curY * ch), float((curX + (wide ? 2 : 1)) * cw), float((curY + 1) * ch),
             curColor);
    }
    node->markDirty(QSGNode::DirtyGeometry);
}

// Images from kitty's graphics protocol: a textured quad each, under the text (z < 0) or over
// it, cut to the item. Textures are kept while an image is in view and made once per version.
void GridView::placeImages(QSGNode* under, QSGNode* over) {
    while (QSGNode* n = under->firstChild()) {
        under->removeChildNode(n);
        delete n;
    }
    while (QSGNode* n = over->firstChild()) {
        over->removeChildNode(n);
        delete n;
    }
    rust::Vec<FfiImage> images;
    kyko_images(quint32(m_pane), images);
    QHash<QPair<quint32, quint64>, QSGTexture*> kept;
    const float w = float(width()), h = float(height());
    for (const FfiImage& im : images) {
        const QPair<quint32, quint64> key(im.image, im.generation);
        QSGTexture* tex = kept.value(key);
        if (!tex)
            tex = m_images.take(key);
        if (!tex) {
            rust::Vec<quint8> px;
            if (im.width == 0 || im.height == 0 || !kyko_image_pixels(quint32(m_pane), im.image, im.generation, px) ||
                px.size() < size_t(im.width) * im.height * 4)
                continue;
            QImage img(px.data(), int(im.width), int(im.height), int(im.width) * 4, QImage::Format_RGBA8888);
            tex = window()->createTextureFromImage(img.copy(), QQuickWindow::TextureHasAlphaChannel);
            if (!tex)
                continue;
            tex->setFiltering(QSGTexture::Linear);
        }
        kept.insert(key, tex);
        if (im.dw == 0 || im.dh == 0 || im.sw == 0 || im.sh == 0)
            continue;
        // Where it goes, then cut to the item, taking the same share off the source.
        float x0 = float(im.col * m_cellW + im.ox), y0 = float(im.row * m_cellH + im.oy);
        float x1 = x0 + float(im.dw), y1 = y0 + float(im.dh);
        float sx0 = float(im.sx), sy0 = float(im.sy), sx1 = sx0 + float(im.sw), sy1 = sy0 + float(im.sh);
        const float kx = (sx1 - sx0) / (x1 - x0), ky = (sy1 - sy0) / (y1 - y0);
        if (x0 < 0) { sx0 -= x0 * kx; x0 = 0; }
        if (y0 < 0) { sy0 -= y0 * ky; y0 = 0; }
        if (x1 > w) { sx1 -= (x1 - w) * kx; x1 = w; }
        if (y1 > h) { sy1 -= (y1 - h) * ky; y1 = h; }
        if (x1 <= x0 || y1 <= y0)
            continue;
        auto* node = new QSGSimpleTextureNode;
        node->setOwnsTexture(false);
        node->setTexture(tex);
        node->setFiltering(QSGTexture::Linear);
        node->setRect(QRectF(x0, y0, x1 - x0, y1 - y0));
        node->setSourceRect(QRectF(sx0, sy0, sx1 - sx0, sy1 - sy0));
        (im.z < 0 ? under : over)->appendChildNode(node);
    }
    // What is no longer in view lets its texture go.
    qDeleteAll(m_images);
    m_images = kept;
}
