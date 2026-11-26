// GridView::updatePaintNode: the frame from Rust turned into scene-graph nodes.
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

QSGNode* GridView::updatePaintNode(QSGNode* old, UpdatePaintNodeData*) {
    rust::Vec<FfiCell> cells;
    rust::Vec<FfiExtra> extras;
    rust::Vec<uint8_t> gutter;
    quint16 cols = 0, rows = 0;
    int cx = -1, cy = -1;
    kyko_snapshot(quint32(m_pane), cells, extras, gutter, cols, rows, cx, cy);
    if (cols == 0 || rows == 0 || cells.size() < size_t(cols) * rows || gutter.size() < size_t(rows)) {
        delete old;
        return nullptr;
    }
    m_curX = cx;
    m_curY = cy;
    const bool focused = hasActiveFocus();
    const bool cursorShown = cx >= 0 && (!m_blink || !focused || m_blinkOn);
    // Only a focused block cursor covers its cell; the rest are drawn over the text.
    const bool block = cursorShown && focused && m_cursorShape == 0;

    QSGNode* root = old;
    if (!root) {
        root = new QSGNode;
        root->appendChildNode(makeBgNode());
        root->appendChildNode(new QSGNode);  // images under the text
        auto* gn = new QSGGeometryNode;
        auto* g = new QSGGeometry(glyphAttributes(), 0);
        g->setDrawingMode(QSGGeometry::DrawTriangles);
        g->setVertexDataPattern(QSGGeometry::StreamPattern);
        gn->setGeometry(g);
        gn->setMaterial(new GlyphMaterial);
        gn->setFlags(QSGNode::OwnsGeometry | QSGNode::OwnsMaterial);
        root->appendChildNode(gn);
        root->appendChildNode(makeBgNode());  // decorations: underlines, strikes, other cursors
        root->appendChildNode(new QSGNode);  // images over the text
    }
    const quint32 cursorColor = kyko_palette_cursor();
    const quint32 bgColor = kyko_palette_bg();
    auto* bg = static_cast<QSGGeometryNode*>(root->firstChild());
    fillBg(bg, cells.data(), gutter.data(), cols, rows, m_cellW, m_cellH, width(), height(),
           block ? cx : -1, cy, cursorColor, m_bgAlpha);

    QHash<quint32, QString> combining;
    for (const FfiExtra& e : extras)
        combining.insert(e.index, QString::fromUtf8(e.text.data(), int(e.text.size())));

    // Glyphs: each row in runs of the same style, broken at spaces, drawn shapes and the cursor
    // (so a ligature never hides the character under it), shaped once per distinct run.
    struct Quad { float x0, y0, x1, y1; Tile t; quint32 color; };
    QList<Quad> quads;
    quads.reserve(int(cols) * rows);
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells.data() + size_t(r) * cols;
        const float rowY = float(r * m_cellH), baseY = float(r * m_cellH + m_ascent);
        auto colorOf = [&](int c) {
            const FfiCell& cell = row[c];
            quint32 fg = (cell.flags & DIM) ? mix(cell.fg, cell.bg, 66) : cell.fg;
            if (block && c == cx && r == cy)
                fg = bgColor;
            return fg;
        };
        int c = 0;
        while (c < cols) {
            const FfiCell& first = row[c];
            if (first.ch <= ' ' || (first.flags & WIDE_SPACER)) {
                ++c;
                continue;
            }
            if (drawnShape(first.ch) && !combining.contains(quint32(r * cols + c))) {
                const Tile t = boxTile(first.ch);
                if (t.valid)
                    quads.append({float(c * m_cellW), rowY, float((c + 1) * m_cellW), float(rowY + m_cellH), t, colorOf(c)});
                ++c;
                continue;
            }
            const quint8 style = first.flags & (BOLD | ITALIC);
            QString text;
            QList<int> cellOf;
            int e = c;
            while (e < cols) {
                const FfiCell& cell = row[e];
                if (cell.flags & WIDE_SPACER) {
                    ++e;
                    continue;
                }
                if (e > c && (cell.ch <= ' ' || (cell.flags & (BOLD | ITALIC)) != style || drawnShape(cell.ch)))
                    break;
                // The cursor's cell is a run of its own.
                if (e > c && r == cy && (e == cx || c == cx))
                    break;
                const char32_t u = cell.ch;
                const QString s = QString::fromUcs4(&u, 1) + combining.value(quint32(r * cols + e));
                for (int k = 0; k < s.size(); ++k)
                    cellOf.append(e - c);
                text += s;
                ++e;
            }
            const QList<Shaped>& glyphs = shape(text, cellOf, style);
            for (const Shaped& g : glyphs) {
                const Tile t = glyphTile(g.font, g.glyph);
                if (!t.valid)
                    continue;
                const int col = c + g.cell;
                const float x0 = float(col * m_cellW + m_shift) + g.dx + t.ox;
                const float y0 = baseY + g.dy + t.oy;
                quads.append({x0, y0, x0 + t.w, y0 + t.h, t, colorOf(std::min<int>(col, cols - 1))});
            }
            c = e;
        }
    }

    QSGNode* under = bg->nextSibling();
    auto* gn = static_cast<QSGGeometryNode*>(under->nextSibling());
    QSGGeometry* g = gn->geometry();
    g->allocate(int(quads.size()) * 6);
    auto* v = static_cast<GlyphVertex*>(g->vertexData());
    for (const Quad& q : quads) {
        const uchar R = (q.color >> 16) & 0xff, G = (q.color >> 8) & 0xff, B = q.color & 0xff;
        // Alpha 0 tells the shader to keep the atlas's own colours (an emoji).
        const uchar A = q.t.color ? 0 : 255;
        const Tile& t = q.t;
        v[0] = {q.x0, q.y0, t.u0, t.v0, R, G, B, A};
        v[1] = {q.x1, q.y0, t.u1, t.v0, R, G, B, A};
        v[2] = {q.x0, q.y1, t.u0, t.v1, R, G, B, A};
        v[3] = {q.x1, q.y0, t.u1, t.v0, R, G, B, A};
        v[4] = {q.x1, q.y1, t.u1, t.v1, R, G, B, A};
        v[5] = {q.x0, q.y1, t.u0, t.v1, R, G, B, A};
        v += 6;
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

    // Decorations: underlines in their five styles and colours, strikethrough, and the cursor
    // when it is a beam, an underline, or an outline (the window does not have the focus).
    struct Rect { float x0, y0, x1, y1; quint32 c; };
    QList<Rect> deco;
    const float th = float(m_ulThick);
    for (int r = 0; r < rows; ++r) {
        const FfiCell* row = cells.data() + size_t(r) * cols;
        const float top = float(r * m_cellH);
        for (int c = 0; c < cols; ++c) {
            const FfiCell& cell = row[c];
            const float x0 = float(c * m_cellW), x1 = float((c + 1) * m_cellW);
            const quint32 fg = (cell.flags & DIM) ? mix(cell.fg, cell.bg, 66) : cell.fg;
            if (cell.underline) {
                const quint32 uc = cell.underline_color ? (cell.underline_color & 0xffffff) : fg;
                const float y = top + float(m_ulPos);
                switch (cell.underline) {
                case UL_DOUBLE:
                    deco.append({x0, y - th, x1, y, uc});
                    deco.append({x0, y + th, x1, y + 2 * th, uc});
                    break;
                case UL_CURLY: {
                    // A wave of one period per cell, drawn one pixel column at a time.
                    const float amp = std::max(1.0f, th * 1.5f);
                    for (int px = 0; px < int(m_cellW); ++px) {
                        const float phase = float(px) / float(m_cellW) * 6.2831853f;
                        const float yy = y + amp * std::sin(phase);
                        deco.append({x0 + px, std::round(yy) - th / 2, x0 + px + 1, std::round(yy) + th / 2 + (th < 2 ? 1 : 0), uc});
                    }
                    break;
                }
                case UL_DOTTED:
                    for (float px = x0; px < x1; px += 2 * th)
                        deco.append({px, y, std::min(x1, px + th), y + th, uc});
                    break;
                case UL_DASHED: {
                    const float dash = std::max(2.0f, float(m_cellW) / 2 - 1);
                    deco.append({x0, y, x0 + dash, y + th, uc});
                    break;
                }
                default:
                    deco.append({x0, y, x1, y + th, uc});
                }
            }
            if (cell.flags & STRIKE)
                deco.append({x0, top + float(m_strikePos), x1, top + float(m_strikePos) + th, fg});
        }
    }
    for (qsizetype i = 0; i + 2 < m_hover.size(); i += 3) {
        const int r = m_hover[i], a = m_hover[i + 1], b = m_hover[i + 2];
        if (r >= rows)
            continue;
        const quint32 fg = cells[size_t(r) * cols + std::min<int>(a, cols - 1)].fg;
        const float y = float(r * m_cellH + m_ulPos);
        deco.append({float(a * m_cellW), y, float(b * m_cellW), y + th, fg});
    }
    if (cursorShown && !block && cx < cols && cy < rows) {
        const bool wide = cells[size_t(cy) * cols + cx].flags & WIDE;
        const float x0 = float(cx * m_cellW), x1 = float((cx + (wide ? 2 : 1)) * m_cellW);
        const float y0 = float(cy * m_cellH), y1 = float((cy + 1) * m_cellH);
        const float t = std::max(1.0f, th);
        if (!focused) {
            deco.append({x0, y0, x1, y0 + t, cursorColor});
            deco.append({x0, y1 - t, x1, y1, cursorColor});
            deco.append({x0, y0, x0 + t, y1, cursorColor});
            deco.append({x1 - t, y0, x1, y1, cursorColor});
        } else if (m_cursorShape == 1) {
            deco.append({x0, y0, x0 + std::max(2.0f, t), y1, cursorColor});
        } else {
            deco.append({x0, y1 - std::max(2.0f, t), x1, y1, cursorColor});
        }
    }
    // See-through rectangles over the text: the visual bell and the scrollbar.
    struct Veil {
        float x0, y0, x1, y1;
        quint32 c;
        uchar a;
    };
    QList<Veil> veils;
    const float pw = float(width()), ph = float(height());
    const quint32 fgColor = kyko_palette_fg();
    if (m_flash)
        veils.append({0, 0, pw, ph, fgColor, 64});
    kyko_scroll_pos(quint32(m_pane), m_scrollOff, m_scrollHist);
    if (m_scrollHist > 0 && (m_scrollOff > 0 || m_scrollDrag)) {
        const float bw = 6;
        const float total = float(m_scrollHist + quint32(rows));
        const float th = std::max(24.0f, ph * float(rows) / total);
        const float top = (ph - th) * (1.0f - float(m_scrollOff) / float(m_scrollHist));
        veils.append({pw - bw, 0, pw, ph, fgColor, 28});
        veils.append({pw - bw, top, pw, top + th, fgColor, m_scrollDrag ? uchar(200) : uchar(140)});
    }
    auto* dn = static_cast<QSGGeometryNode*>(gn->nextSibling());
    QSGGeometry* dg = dn->geometry();
    dg->allocate(int(deco.size() + veils.size()) * 6);
    auto* dp = dg->vertexDataAsColoredPoint2D();
    for (const Rect& d : deco)
        quad(dp, d.x0, d.y0, d.x1, d.y1, d.c);
    for (const Veil& v : veils) {
        // Premultiplied, as the vertex colour material expects.
        const uchar r = uchar(((v.c >> 16) & 0xff) * v.a / 255), g = uchar(((v.c >> 8) & 0xff) * v.a / 255),
                    b = uchar((v.c & 0xff) * v.a / 255);
        dp[0].set(v.x0, v.y0, r, g, b, v.a);
        dp[1].set(v.x1, v.y0, r, g, b, v.a);
        dp[2].set(v.x0, v.y1, r, g, b, v.a);
        dp[3].set(v.x1, v.y0, r, g, b, v.a);
        dp[4].set(v.x1, v.y1, r, g, b, v.a);
        dp[5].set(v.x0, v.y1, r, g, b, v.a);
        dp += 6;
    }
    dn->markDirty(QSGNode::DirtyGeometry);
    placeImages(under, dn->nextSibling());

    if (m_atlasReset) {
        m_atlasReset = false;
        update();
    }
    return root;
}
