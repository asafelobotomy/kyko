// The glyph atlas: glyphs and drawn shapes rasterised once into one texture, and shaped text
// runs cached by their text and style.
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

bool GridView::allocate(int w, int h, int& x, int& y) {
    if (w > ATLAS || h > ATLAS)
        return false;
    if (m_shelfX + w > ATLAS) {
        m_shelfY += m_shelfH;
        m_shelfX = 0;
        m_shelfH = 0;
    }
    if (m_shelfY + h > ATLAS)
        return false;
    x = m_shelfX;
    y = m_shelfY;
    m_shelfX += w;
    m_shelfH = std::max(m_shelfH, h);
    return true;
}

int GridView::fontIndex(const QRawFont& f) {
    for (int i = 0; i < m_rawFonts.size(); ++i)
        if (m_rawFonts[i] == f)
            return i;
    m_rawFonts.append(f);
    return int(m_rawFonts.size()) - 1;
}

GridView::Tile GridView::glyphTile(int font, quint32 glyph) {
    const quint64 key = (quint64(font) << 32) | glyph;
    auto it = m_tiles.constFind(key);
    if (it != m_tiles.constEnd())
        return *it;
    Tile t;
    const QRawFont& rf = m_rawFonts[font];
    const QRectF br = rf.boundingRect(glyph);
    if (br.isEmpty()) {
        m_tiles.insert(key, t);
        return t;
    }
    const int ox = int(std::floor(br.left())) - 1, oy = int(std::floor(br.top())) - 1;
    const int w = int(std::ceil(br.right())) + 1 - ox, h = int(std::ceil(br.bottom())) + 1 - oy;
    int x, y;
    if (!allocate(w, h, x, y)) {
        // The atlas is full: start it again, and draw everything again next frame.
        resetAtlas();
        m_atlasReset = true;
        if (!allocate(w, h, x, y))
            return t;
    }
    {
        QPainter p(&m_atlas);
        p.setCompositionMode(QPainter::CompositionMode_Source);
        p.fillRect(x, y, w, h, Qt::transparent);
        p.setCompositionMode(QPainter::CompositionMode_SourceOver);
        p.setPen(Qt::white);
        QGlyphRun run;
        run.setRawFont(rf);
        run.setGlyphIndexes({glyph});
        run.setPositions({QPointF(0, 0)});
        p.drawGlyphRun(QPointF(x - ox, y - oy), run);
    }
    // A glyph drawn in white has equal red, green, blue and alpha everywhere; anything else is a
    // colour glyph (emoji), drawn with its own colours.
    bool color = false;
    for (int yy = y; yy < y + h && !color; ++yy) {
        const QRgb* line = reinterpret_cast<const QRgb*>(m_atlas.constScanLine(yy));
        for (int xx = x; xx < x + w; ++xx) {
            const QRgb px = line[xx];
            const int a = qAlpha(px);
            if (qRed(px) != a || qGreen(px) != a || qBlue(px) != a) {
                color = true;
                break;
            }
        }
    }
    t = {float(x) / ATLAS, float(y) / ATLAS, float(x + w) / ATLAS, float(y + h) / ATLAS,
         float(ox), float(oy), float(w), float(h), color, true};
    m_tiles.insert(key, t);
    m_atlasDirty = true;
    return t;
}

GridView::Tile GridView::boxTile(quint32 ch) {
    const quint64 key = (quint64(0xffff) << 32) | ch;
    auto it = m_tiles.constFind(key);
    if (it != m_tiles.constEnd())
        return *it;
    Tile t;
    const int w = int(m_cellW), h = int(m_cellH);
    int x, y;
    if (!allocate(w, h, x, y)) {
        resetAtlas();
        m_atlasReset = true;
        if (!allocate(w, h, x, y))
            return t;
    }
    QPainter p(&m_atlas);
    p.setCompositionMode(QPainter::CompositionMode_Source);
    p.fillRect(x, y, w, h, Qt::transparent);
    p.setCompositionMode(QPainter::CompositionMode_SourceOver);
    p.setClipRect(x, y, w, h);
    if (!drawShape(p, ch, x, y, w, h, int(m_ulThick)))
        return t;
    t = {float(x) / ATLAS, float(y) / ATLAS, float(x + w) / ATLAS, float(y + h) / ATLAS,
         0, 0, float(w), float(h), false, true};
    m_tiles.insert(key, t);
    m_atlasDirty = true;
    return t;
}

const QList<GridView::Shaped>& GridView::shape(const QString& text, const QList<int>& cellOf, quint8 style) {
    const QString key = QChar(u'0' + style) + text;
    auto it = m_shaped.constFind(key);
    if (it != m_shaped.constEnd())
        return *it;
    if (m_shaped.size() > 8192)
        m_shaped.clear();
    QList<Shaped> out;
    QTextLayout layout(text, m_fonts[style & 3]);
    QTextOption opt;
    opt.setWrapMode(QTextOption::NoWrap);
    layout.setTextOption(opt);
    layout.setCacheEnabled(true);
    layout.beginLayout();
    QTextLine line = layout.createLine();
    line.setLineWidth(1e7);
    layout.endLayout();
    const qreal base = line.ascent();
    struct G { int font; quint32 glyph; int str; QPointF pos; };
    QList<G> all;
    for (const QGlyphRun& run : layout.glyphRuns(-1, -1, QTextLayout::RetrieveAll)) {
        const int font = fontIndex(run.rawFont());
        const QList<quint32> glyphs = run.glyphIndexes();
        const QList<QPointF> pos = run.positions();
        const QList<qsizetype> str = run.stringIndexes();
        for (qsizetype i = 0; i < glyphs.size(); ++i)
            all.append({font, glyphs[i], int(i < str.size() ? str[i] : 0), pos[i]});
    }
    // Each cell's pen position: the first glyph that belongs to it.
    QHash<int, qreal> penX;
    std::stable_sort(all.begin(), all.end(), [](const G& a, const G& b) { return a.str < b.str; });
    for (const G& g : all) {
        const int cell = cellOf.value(g.str, 0);
        if (!penX.contains(cell))
            penX.insert(cell, g.pos.x());
    }
    for (const G& g : all) {
        const int cell = cellOf.value(g.str, 0);
        out.append({g.font, g.glyph, cell, float(g.pos.x() - penX.value(cell)), float(g.pos.y() - base)});
    }
    return *m_shaped.insert(key, out);
}
