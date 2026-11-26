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

// ---- the item -----------------------------------------------------------------------------------
GridView::GridView(QQuickItem* parent) : QQuickItem(parent) {
    setFlag(ItemHasContents, true);
    setFlag(ItemAcceptsInputMethod, true);
    setActiveFocusOnTab(true);
    setAcceptedMouseButtons(Qt::LeftButton | Qt::MiddleButton | Qt::RightButton);
    setAcceptHoverEvents(true);
    setCursor(Qt::IBeamCursor);
    g_views.append(this);
    m_blinkTimer.setInterval(530);
    QObject::connect(&m_blinkTimer, &QTimer::timeout, this, [this] {
        m_blinkOn = !m_blinkOn;
        update();
    });
    m_atlas = QImage(ATLAS, ATLAS, QImage::Format_ARGB32_Premultiplied);
    applyFont();
    resetAtlas();
}

namespace {
// The configured font, before the ligature features are set.
QFont lookFont() {
    QFont base(QString::fromStdString(std::string(kyko_palette_font())));
    base.setPointSize(kyko_palette_font_size());
    base.setStyleHint(QFont::Monospace);
    base.setFixedPitch(true);
    base.setKerning(false);
    return base;
}

// A cell: whole pixels, as wide as an M and as tall as the line height makes it.
QSizeF cellOf(const QFontMetricsF& fm) {
    const int pct = std::clamp(kyko_setting(rust::Str("line_height_pct")), 80, 200);
    return QSizeF(std::ceil(fm.horizontalAdvance(QLatin1Char('M'))), std::ceil(fm.lineSpacing() * pct / 100.0));
}
}  // namespace

quint32 grid_cell_px() {
    const QSizeF c = cellOf(QFontMetricsF(lookFont()));
    return (quint32(c.width()) << 16) | (quint32(c.height()) & 0xffff);
}

void GridView::applyFont() {
    QFont base = lookFont();
    m_ligatures = kyko_setting(rust::Str("ligatures")) != 0;
    if (!m_ligatures) {
        // The same features kitty turns off for `disable_ligatures always`.
        base.setFeature(QFont::Tag("liga"), 0);
        base.setFeature(QFont::Tag("calt"), 0);
        base.setFeature(QFont::Tag("dlig"), 0);
    }
    for (int i = 0; i < 4; ++i) {
        m_fonts[i] = base;
        m_fonts[i].setBold(i & 1);
        m_fonts[i].setItalic(i & 2);
    }
    m_font = base;
    QFontMetricsF fm(base);
    const qreal natural = fm.horizontalAdvance(QLatin1Char('M'));
    const QSizeF cell = cellOf(fm);
    m_cellW = cell.width();
    // Narrow glyphs sit in the middle of the whole-pixel cell.
    m_shift = std::floor((m_cellW - natural) / 2);
    m_cellH = cell.height();
    m_ascent = std::round(fm.ascent() + (m_cellH - fm.height()) / 2);
    m_ulThick = std::max<qreal>(1, std::round(fm.lineWidth()));
    m_ulPos = std::min(m_cellH - m_ulThick, std::round(m_ascent + std::max<qreal>(1, fm.underlinePos())));
    m_strikePos = std::round(m_ascent - fm.strikeOutPos());
    m_cursorShape = std::clamp(kyko_setting(rust::Str("cursor")), 0, 2);
    m_blink = kyko_setting(rust::Str("cursor_blink")) != 0;
    m_bgAlpha = uchar(std::clamp(kyko_setting(rust::Str("opacity_pct")), 30, 100) * 255 / 100);
    m_blinkOn = true;
    if (m_blink && hasActiveFocus())
        m_blinkTimer.start();
    else
        m_blinkTimer.stop();
}

void GridView::resetAtlas() {
    m_atlas.fill(Qt::transparent);
    m_tiles.clear();
    m_shelfX = m_shelfY = m_shelfH = 0;
    m_atlasDirty = true;
}

void GridView::reloadLook() {
    applyFont();
    m_shaped.clear();
    m_rawFonts.clear();
    resetAtlas();
    fit();
    update();
}

void GridView::setPane(int pane) {
    if (pane == m_pane)
        return;
    m_pane = pane;
    // The size may be known already: this pane has to hear it.
    m_cols = m_rows = 0;
    fit();
    Q_EMIT paneChanged();
    update();
}

void GridView::fit() {
    // Images are sized in pixels: the pane learns its cell's size.
    const int cw = int(std::round(m_cellW)), ch = int(std::round(m_cellH));
    if (cw != m_sentCellW || ch != m_sentCellH) {
        m_sentCellW = cw;
        m_sentCellH = ch;
        kyko_cell_px(quint16(cw), quint16(ch));
    }
    const int cols = int(width() / m_cellW), rows = int(height() / m_cellH);
    if (cols < 2 || rows < 1 || (cols == m_cols && rows == m_rows))
        return;
    m_cols = cols;
    m_rows = rows;
    if (m_pane > 0)
        kyko_resize(quint32(m_pane), quint16(cols), quint16(rows));
    Q_EMIT gridChanged();
}

GridView::~GridView() {
    g_views.removeAll(this);
    delete m_texture;
    qDeleteAll(m_images);
}

QString GridView::shot() const {
    const auto s = kyko_test_shot();
    return QString::fromUtf8(s.data(), int(s.size()));
}

void GridView::geometryChange(const QRectF& n, const QRectF& o) {
    QQuickItem::geometryChange(n, o);
    fit();
    update();
}

void GridView::bell(int mode) {
    if (mode <= 0)
        return;
    if (QQuickWindow* w = window(); w && !w->isActive())
        w->alert(0);
    if (mode == 2) {
        // libcanberra plays the theme's bell through the desktop's sound server; without it
        // the bell is silent rather than a flash nobody asked for.
        QProcess::startDetached(QStringLiteral("canberra-gtk-play"),
                                {QStringLiteral("-i"), QStringLiteral("bell"), QStringLiteral("-d"),
                                 QStringLiteral("kyko")});
        return;
    }
    if (m_flash)
        return;
    m_flash = true;
    update();
    QTimer::singleShot(120, this, [this] {
        m_flash = false;
        update();
    });
}

void app_set_translucent() {
    QQuickWindow::setDefaultAlphaBuffer(true);
}

void app_set_id(rust::Str id) {
    QGuiApplication::setDesktopFileName(QString::fromUtf8(id.data(), int(id.size())));
}
