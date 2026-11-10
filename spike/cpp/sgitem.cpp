#include "sgitem.h"

#include <QtCore/QElapsedTimer>
#include <QtCore/QPointer>
#include <QtGui/QFontMetricsF>
#include <QtGui/QGuiApplication>
#include <QtGui/QPainter>
#include <QtGui/QTextLayout>
#include <QtQuick/QQuickWindow>
#include <QtQuick/QSGGeometryNode>
#include <QtQuick/QSGMaterial>
#include <QtQuick/QSGMaterialShader>
#include <QtQuick/QSGTextNode>
#include <QtQuick/QSGTexture>
#include <QtQuick/QSGVertexColorMaterial>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <vector>

#include "griditem.h"
#include "kyko-spike/src/bridge.cxx.h"

namespace {
const quint8 BOLD = 1, ITALIC = 2, UNDERLINE = 4, WIDE = 16, WIDE_SPACER = 32;
const int ATLAS = 2048;

void report(const char* name, std::vector<double>& v) {
    if (v.empty())
        return;
    std::sort(v.begin(), v.end());
    auto at = [&](double q) { return v[std::min(v.size() - 1, size_t(q * v.size()))]; };
    std::fprintf(stderr, " %s p50=%.2f p95=%.2f max=%.2f", name, at(0.50), at(0.95), v.back());
    v.clear();
}

// ---- glyph material: alpha from the atlas, colour per vertex -------------------------------------
struct GlyphVertex {
    float x, y, u, v;
    uchar r, g, b, a;
};

class GlyphMaterial : public QSGMaterial {
public:
    GlyphMaterial() { setFlag(Blending, true); }
    QSGMaterialType* type() const override {
        static QSGMaterialType t;
        return &t;
    }
    QSGMaterialShader* createShader(QSGRendererInterface::RenderMode) const override;
    int compare(const QSGMaterial* o) const override {
        return texture == static_cast<const GlyphMaterial*>(o)->texture ? 0 : 1;
    }
    QSGTexture* texture = nullptr;
};

class GlyphShader : public QSGMaterialShader {
public:
    GlyphShader() {
        setShaderFileName(VertexStage, QStringLiteral(KYKO_SHADER_DIR "/glyph.vert.qsb"));
        setShaderFileName(FragmentStage, QStringLiteral(KYKO_SHADER_DIR "/glyph.frag.qsb"));
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

inline void quad(QSGGeometry::ColoredPoint2D*& p, float x0, float y0, float x1, float y1, quint32 c) {
    const uchar r = (c >> 16) & 0xff, g = (c >> 8) & 0xff, b = c & 0xff;
    p[0].set(x0, y0, r, g, b, 255);
    p[1].set(x1, y0, r, g, b, 255);
    p[2].set(x0, y1, r, g, b, 255);
    p[3].set(x1, y0, r, g, b, 255);
    p[4].set(x1, y1, r, g, b, 255);
    p[5].set(x0, y1, r, g, b, 255);
    p += 6;
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

void fillBg(QSGGeometryNode* node, const FfiCell* cells, int cols, int rows, qreal cw, qreal ch, qreal w, qreal h) {
    const quint32 def = spike_palette_bg();
    int quads = 1;
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
    quad(p, 0, 0, float(w), float(h), def);
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
    node->markDirty(QSGNode::DirtyGeometry);
}

// ---- whole-frame timing --------------------------------------------------------------------------
struct Frame {
    QElapsedTimer clock;
    qint64 syncStart = 0, renderStart = 0, lastSwap = 0;
    int warm = 0;
    std::vector<double> sync, render, interval;
};
}  // namespace

void frame_stats_attach(QQuickWindow* w) {
    if (!w || !spike_stats_enabled() || w->property("kykoStats").toBool())
        return;
    w->setProperty("kykoStats", true);
    auto* f = new Frame;
    f->clock.start();
    const auto d = Qt::DirectConnection;
    QObject::connect(w, &QQuickWindow::beforeSynchronizing, w, [f] { f->syncStart = f->clock.nsecsElapsed(); }, d);
    QObject::connect(w, &QQuickWindow::afterSynchronizing, w, [f] {
        if (f->warm >= 60)
            f->sync.push_back((f->clock.nsecsElapsed() - f->syncStart) / 1e6);
    }, d);
    QObject::connect(w, &QQuickWindow::beforeRendering, w, [f] { f->renderStart = f->clock.nsecsElapsed(); }, d);
    QObject::connect(w, &QQuickWindow::afterRendering, w, [f] {
        if (f->warm >= 60)
            f->render.push_back((f->clock.nsecsElapsed() - f->renderStart) / 1e6);
    }, d);
    QObject::connect(w, &QQuickWindow::frameSwapped, w, [f] {
        const qint64 now = f->clock.nsecsElapsed();
        if (f->warm >= 60 && f->lastSwap)
            f->interval.push_back((now - f->lastSwap) / 1e6);
        f->lastSwap = now;
        if (++f->warm < 60)
            return;
        if (f->interval.size() >= 300) {
            std::fprintf(stderr, "frame_ms n=300");
            report("sync", f->sync);
            report("render", f->render);
            report("interval", f->interval);
            std::fprintf(stderr, "\n");
        }
    }, d);
}

// ---- the item ------------------------------------------------------------------------------------
SgItem::SgItem(QQuickItem* parent) : QQuickItem(parent) {
    setFlag(ItemHasContents, true);
    grid_set_sg(this);
    m_textMode = qgetenv("KYKO_SPIKE_RENDERER") == "text";
    m_full = qEnvironmentVariableIsSet("KYKO_SPIKE_FULL");
    m_font = QFont(QString::fromStdString(std::string(spike_palette_font())));
    m_font.setPointSize(spike_palette_font_size());
    m_font.setStyleHint(QFont::Monospace);
    m_font.setFixedPitch(true);
    m_font.setKerning(false);
    QFontMetricsF fm(m_font);
    const qreal natural = fm.horizontalAdvance(QLatin1Char('M'));
    m_cellW = std::ceil(natural);
    m_font.setLetterSpacing(QFont::AbsoluteSpacing, m_cellW - natural);
    m_cellH = std::ceil(fm.lineSpacing());
    m_ascent = fm.ascent() + (m_cellH - fm.height()) / 2;
    m_atlas = QImage(ATLAS, ATLAS, QImage::Format_ARGB32_Premultiplied);
    m_atlas.fill(Qt::transparent);
}

void SgItem::itemChange(ItemChange change, const ItemChangeData& data) {
    QQuickItem::itemChange(change, data);
    if (change == ItemSceneChange && data.window)
        frame_stats_attach(data.window);
}

void SgItem::geometryChange(const QRectF& n, const QRectF& o) {
    QQuickItem::geometryChange(n, o);
    const int cols = int(n.width() / m_cellW), rows = int(n.height() / m_cellH);
    if (cols < 2 || rows < 1 || (cols == m_cols && rows == m_rows))
        return;
    m_cols = cols;
    m_rows = rows;
    if (!m_started) {
        m_started = true;
        spike_start(quint16(cols), quint16(rows));
    } else {
        spike_resize(quint16(cols), quint16(rows));
    }
    Q_EMIT gridChanged();
    update();
}

SgItem::Slot SgItem::slotFor(quint32 ch, quint8 style) {
    const quint64 key = quint64(ch) | (quint64(style) << 32);
    auto it = m_slots.constFind(key);
    if (it != m_slots.constEnd())
        return *it;
    const int cw = int(m_cellW), chh = int(m_cellH);
    const int perRow = ATLAS / cw;
    QFontMetricsF fm(m_font);
    const char32_t u = ch;
    const QString s = QString::fromUcs4(&u, 1);
    const bool wide = style & WIDE;
    const int span = wide ? 2 : 1;
    if (m_nextSlot % perRow + span > perRow)
        m_nextSlot += perRow - m_nextSlot % perRow;
    const int sx = (m_nextSlot % perRow) * cw, sy = (m_nextSlot / perRow) * chh;
    m_nextSlot += span;
    Slot slot{0, 0, 0, 0, wide};
    if (sy + chh <= ATLAS) {
        QPainter p(&m_atlas);
        QFont f = m_font;
        f.setBold(style & BOLD);
        f.setItalic(style & ITALIC);
        f.setUnderline(style & UNDERLINE);
        p.setFont(f);
        p.setPen(Qt::white);
        p.setClipRect(sx, sy, cw * span, chh);
        p.drawText(QPointF(sx, sy + m_ascent), s);
        p.end();
        slot = {float(sx) / ATLAS, float(sy) / ATLAS, float(sx + cw * span) / ATLAS, float(sy + chh) / ATLAS, wide};
        m_atlasDirty = true;
    }
    m_slots.insert(key, slot);
    return slot;
}

QSGNode* SgItem::updatePaintNode(QSGNode* old, UpdatePaintNodeData*) {
    QElapsedTimer timer;
    timer.start();

    rust::Vec<FfiCell> cells;
    quint16 cols = 0, rows = 0;
    int cx = -1, cy = -1;
    spike_snapshot(cells, cols, rows, cx, cy);

    QSGNode* root = old;
    if (!root) {
        root = new QSGNode;
        root->appendChildNode(makeBgNode());
        if (!m_textMode) {
            auto* gn = new QSGGeometryNode;
            auto* g = new QSGGeometry(glyphAttributes(), 0);
            g->setDrawingMode(QSGGeometry::DrawTriangles);
            g->setVertexDataPattern(QSGGeometry::StreamPattern);
            gn->setGeometry(g);
            gn->setMaterial(new GlyphMaterial);
            gn->setFlags(QSGNode::OwnsGeometry | QSGNode::OwnsMaterial);
            root->appendChildNode(gn);
        }
    }
    auto* bg = static_cast<QSGGeometryNode*>(root->firstChild());
    fillBg(bg, cells.data(), cols, rows, m_cellW, m_cellH, width(), height());

    if (!m_textMode) {
        auto* gn = static_cast<QSGGeometryNode*>(bg->nextSibling());
        int count = 0;
        for (const FfiCell& c : cells)
            if (c.ch > ' ' && !(c.flags & WIDE_SPACER))
                ++count;
        QSGGeometry* g = gn->geometry();
        g->allocate(count * 6);
        auto* v = static_cast<GlyphVertex*>(g->vertexData());
        for (int r = 0; r < rows; ++r) {
            const FfiCell* row = cells.data() + size_t(r) * cols;
            const float y0 = float(r * m_cellH), y1 = float((r + 1) * m_cellH);
            for (int c = 0; c < cols; ++c) {
                const FfiCell& cell = row[c];
                if (cell.ch <= ' ' || (cell.flags & WIDE_SPACER))
                    continue;
                const Slot s = slotFor(cell.ch, cell.flags & (BOLD | ITALIC | UNDERLINE | WIDE));
                const float x0 = float(c * m_cellW), x1 = float((c + (s.wide ? 2 : 1)) * m_cellW);
                const uchar R = (cell.fg >> 16) & 0xff, G = (cell.fg >> 8) & 0xff, B = cell.fg & 0xff;
                v[0] = {x0, y0, s.u0, s.v0, R, G, B, 255};
                v[1] = {x1, y0, s.u1, s.v0, R, G, B, 255};
                v[2] = {x0, y1, s.u0, s.v1, R, G, B, 255};
                v[3] = {x1, y0, s.u1, s.v0, R, G, B, 255};
                v[4] = {x1, y1, s.u1, s.v1, R, G, B, 255};
                v[5] = {x0, y1, s.u0, s.v1, R, G, B, 255};
                v += 6;
            }
        }
        auto* mat = static_cast<GlyphMaterial*>(gn->material());
        if (m_atlasDirty || !m_texture) {
            delete m_texture;
            m_texture = window()->createTextureFromImage(m_atlas, QQuickWindow::TextureHasAlphaChannel);
            m_texture->setFiltering(QSGTexture::Nearest);
            mat->texture = m_texture;
            m_atlasDirty = false;
            gn->markDirty(QSGNode::DirtyMaterial);
        }
        gn->markDirty(QSGNode::DirtyGeometry);
    } else {
        // One QSGTextNode per row; a row is laid out again only when its contents changed.
        while (root->childCount() - 1 < rows) {
            QSGTextNode* t = window()->createTextNode();
            if (qEnvironmentVariableIsSet("KYKO_SPIKE_NATIVE_TEXT"))
                t->setRenderType(QSGTextNode::NativeRendering);
            root->appendChildNode(t);
        }
        if (m_rowHash.size() != rows)
            m_rowHash = QList<quint64>(rows, 0);
        const QColor def = QColor::fromRgb(spike_palette_fg());
        QSGNode* n = bg->nextSibling();
        for (int r = 0; r < rows; ++r, n = n->nextSibling()) {
            const FfiCell* row = cells.data() + size_t(r) * cols;
            quint64 h = 1469598103934665603ull;
            for (int c = 0; c < cols; ++c) {
                h = (h ^ row[c].ch) * 1099511628211ull;
                h = (h ^ row[c].fg) * 1099511628211ull;
                h = (h ^ row[c].flags) * 1099511628211ull;
            }
            if (!m_full && h == m_rowHash[r])
                continue;
            m_rowHash[r] = h;
            auto* t = static_cast<QSGTextNode*>(n);
            t->clear();
            QString text;
            QList<QTextLayout::FormatRange> formats;
            for (int c = 0; c < cols;) {
                if (row[c].flags & WIDE_SPACER) {
                    ++c;
                    continue;
                }
                const quint32 fg = row[c].fg;
                const quint8 style = row[c].flags & (BOLD | ITALIC | UNDERLINE);
                const int start = text.size();
                int e = c;
                while (e < cols && row[e].fg == fg && (row[e].flags & (BOLD | ITALIC | UNDERLINE)) == style) {
                    if (!(row[e].flags & WIDE_SPACER)) {
                        const char32_t u = row[e].ch ? row[e].ch : U' ';
                        text.append(QString::fromUcs4(&u, 1));
                    }
                    ++e;
                }
                QTextCharFormat f;
                f.setForeground(QColor::fromRgb(fg));
                if (style & BOLD)
                    f.setFontWeight(QFont::Bold);
                if (style & ITALIC)
                    f.setFontItalic(true);
                if (style & UNDERLINE)
                    f.setFontUnderline(true);
                formats.append({start, int(text.size() - start), f});
                c = e;
            }
            QTextLayout layout(text, m_font);
            layout.setFormats(formats);
            layout.setCacheEnabled(true);
            layout.beginLayout();
            QTextLine line = layout.createLine();
            if (line.isValid()) {
                line.setLineWidth(1e6);
                line.setPosition(QPointF(0, 0));
            }
            layout.endLayout();
            t->setColor(def);
            t->addTextLayout(QPointF(0, r * m_cellH), &layout);
        }
    }

    if (spike_stats_enabled()) {
        m_buildMs.push_back(timer.nsecsElapsed() / 1e6);
        if (m_buildMs.size() >= 300) {
            std::fprintf(stderr, "build_ms n=300");
            report("build", m_buildMs);
            std::fprintf(stderr, " cols=%d rows=%d glyph_slots=%d\n", int(cols), int(rows), int(m_slots.size()));
        }
    }
    if (m_full)
        QMetaObject::invokeMethod(this, [this] { update(); }, Qt::QueuedConnection);
    return root;
}
