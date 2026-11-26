// Drawn shapes: box drawing, blocks and Powerline separators, sized to the cell.
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

namespace {
// Box drawing U+2500..U+257F as four arms (up, right, down, left): 0 none, 1 light, 2 heavy.
// 0 marks a character drawn some other way (dashes, double lines, arcs, diagonals).
const char* const BOX_ARMS[128] = {
    "0101", "0202", "1010", "2020", nullptr, nullptr, nullptr, nullptr,  // 2500
    nullptr, nullptr, nullptr, nullptr, "0110", "0210", "0120", "0220",  // 2508
    "0011", "0012", "0021", "0022", "1100", "1200", "2100", "2200",      // 2510
    "1001", "1002", "2001", "2002", "1110", "1210", "2110", "1120",      // 2518
    "2120", "2210", "1220", "2220", "1011", "1012", "2011", "1021",      // 2520
    "2021", "2012", "1022", "2022", "0111", "0112", "0211", "0212",      // 2528
    "0121", "0122", "0221", "0222", "1101", "1102", "1201", "1202",      // 2530
    "2101", "2102", "2201", "2202", "1111", "1112", "1211", "1212",      // 2538
    "2111", "1121", "2121", "2112", "2211", "1122", "1221", "2212",      // 2540
    "1222", "2122", "2221", "2222", nullptr, nullptr, nullptr, nullptr,  // 2548
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2550
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2558
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2560
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,  // 2568
    nullptr, nullptr, nullptr, nullptr, "0001", "1000", "0100", "0010",  // 2570
    "0002", "2000", "0200", "0020", "0201", "2010", "0102", "1020",      // 2578
};

// Double lines U+2550..U+256C as arms: 0 none, 1 single, 3 double.
const char* const DOUBLE_ARMS[29] = {
    "0303", "3030", "0310", "0130", "0330", "0013", "0031", "0033",  // 2550
    "1300", "3100", "3300", "1003", "3001", "3003", "1310", "3130",  // 2558
    "3330", "1013", "3031", "3033", "0313", "0131", "0333", "1303",  // 2560
    "3101", "3303", "1313", "3131", "3333",                          // 2568
};
}  // namespace

bool drawnShape(quint32 ch) {
    return (ch >= 0x2500 && ch <= 0x259f) || (ch >= 0xe0b0 && ch <= 0xe0bf);
}

bool drawShape(QPainter& p, quint32 ch, int x, int y, int w, int h, int light) {
    const int heavy = std::max(light + 1, light * 2);
    const int cx = x + w / 2, cy = y + h / 2;
    const QColor white(Qt::white);
    auto thick = [&](char a) { return a == '2' ? heavy : a == '1' ? light : 0; };
    auto armsDraw = [&](const char* a) {
        const int up = thick(a[0]), right = thick(a[1]), down = thick(a[2]), left = thick(a[3]);
        const int vt = std::max(up, down), ht = std::max(left, right);
        if (up) p.fillRect(cx - up / 2, y, up, cy - y + (ht + 1) / 2 + (ht ? 0 : up / 2 + 1), white);
        if (down) p.fillRect(cx - down / 2, cy - (ht ? ht / 2 : down / 2), down, y + h - cy + (ht ? ht / 2 : down / 2), white);
        if (left) p.fillRect(x, cy - left / 2, cx - x + (vt + 1) / 2 + (vt ? 0 : left / 2 + 1), left, white);
        if (right) p.fillRect(cx - (vt ? vt / 2 : right / 2), cy - right / 2, x + w - cx + (vt ? vt / 2 : right / 2), right, white);
    };
    if (ch >= 0x2500 && ch <= 0x257f) {
        const int i = int(ch - 0x2500);
        if (BOX_ARMS[i]) {
            armsDraw(BOX_ARMS[i]);
            return true;
        }
        // Dashed lines: 2504-250b (triple, quadruple), 254c-254f (double).
        auto dashes = [&](bool vertical, int n, int t) {
            const int len = vertical ? h : w;
            const qreal seg = qreal(len) / n;
            for (int k = 0; k < n; ++k) {
                const int a = int(std::round(k * seg)), b = int(std::round(k * seg + seg * 0.6));
                if (vertical)
                    p.fillRect(cx - t / 2, y + a, t, std::max(1, b - a), white);
                else
                    p.fillRect(x + a, cy - t / 2, std::max(1, b - a), t, white);
            }
        };
        switch (ch) {
        case 0x2504: dashes(false, 3, light); return true;
        case 0x2505: dashes(false, 3, heavy); return true;
        case 0x2506: dashes(true, 3, light); return true;
        case 0x2507: dashes(true, 3, heavy); return true;
        case 0x2508: dashes(false, 4, light); return true;
        case 0x2509: dashes(false, 4, heavy); return true;
        case 0x250a: dashes(true, 4, light); return true;
        case 0x250b: dashes(true, 4, heavy); return true;
        case 0x254c: dashes(false, 2, light); return true;
        case 0x254d: dashes(false, 2, heavy); return true;
        case 0x254e: dashes(true, 2, light); return true;
        case 0x254f: dashes(true, 2, heavy); return true;
        default: break;
        }
        if (ch >= 0x2550 && ch <= 0x256c) {
            // Double lines: two strokes `gap` apart; at a junction each stroke stops at the
            // stroke it meets, so corners and tees close the way the font's would.
            const char* a = DOUBLE_ARMS[ch - 0x2550];
            const int g = std::max(light + 1, light * 2);  // centre to stroke
            const int t = light;
            auto hline = [&](int x0, int x1, int yy) { if (x1 > x0) p.fillRect(x0, yy - t / 2, x1 - x0, t, white); };
            auto vline = [&](int y0, int y1, int xx) { if (y1 > y0) p.fillRect(xx - t / 2, y0, t, y1 - y0, white); };
            const char up = a[0], right = a[1], down = a[2], left = a[3];
            const bool vDouble = up == '3' || down == '3', hDouble = left == '3' || right == '3';
            const int vr = vDouble ? g : 0;  // half-width of the vertical strokes
            const int hr = hDouble ? g : 0;
            // Horizontal arms.
            for (int side : {-1, 1}) {
                const char arm = side < 0 ? left : right;
                if (arm == '0')
                    continue;
                const int edge = side < 0 ? x : x + w;
                if (arm == '1') {
                    const int stop = cx + side * -vr;  // single meets the near vertical stroke
                    side < 0 ? hline(edge, stop + (vr ? 0 : (t + 1) / 2), cy) : hline(stop - (vr ? 0 : t / 2), edge, cy);
                    continue;
                }
                for (int lane : {-1, 1}) {
                    const int yy = cy + lane * g;
                    // The stroke on the side where the vertical arm comes from stops at the near
                    // vertical stroke; the other runs to the far one (or the centre).
                    const bool vertHere = (lane < 0 ? up : down) != '0';
                    const bool vertOther = (lane < 0 ? down : up) != '0';
                    int reach;
                    if (vertHere)
                        reach = -vr;  // stop at the near vertical stroke
                    else if (vertOther || vDouble)
                        reach = vr;   // run to the far stroke
                    else
                        reach = up == '1' || down == '1' ? 0 : 0;
                    if (!vertHere && !vertOther && !vDouble)
                        reach = (up == '0' && down == '0') ? (side < 0 ? (w - w / 2) : w / 2) : 0;
                    const int stop = cx - side * reach;
                    side < 0 ? hline(edge, stop + (t + 1) / 2, yy) : hline(stop - t / 2, edge, yy);
                }
            }
            for (int side : {-1, 1}) {
                const char arm = side < 0 ? up : down;
                if (arm == '0')
                    continue;
                const int edge = side < 0 ? y : y + h;
                if (arm == '1') {
                    const int stop = cy + side * -hr;
                    side < 0 ? vline(edge, stop + (hr ? 0 : (t + 1) / 2), cx) : vline(stop - (hr ? 0 : t / 2), edge, cx);
                    continue;
                }
                for (int lane : {-1, 1}) {
                    const int xx = cx + lane * g;
                    const bool horHere = (lane < 0 ? left : right) != '0';
                    const bool horOther = (lane < 0 ? right : left) != '0';
                    int reach;
                    if (horHere)
                        reach = -hr;
                    else if (horOther || hDouble)
                        reach = hr;
                    else
                        reach = (left == '0' && right == '0') ? (side < 0 ? (h - h / 2) : h / 2) : 0;
                    const int stop = cy - side * reach;
                    side < 0 ? vline(edge, stop + (t + 1) / 2, xx) : vline(stop - t / 2, edge, xx);
                }
            }
            return true;
        }
        p.save();
        p.setRenderHint(QPainter::Antialiasing, true);
        QPen pen(white, light, Qt::SolidLine, Qt::FlatCap, Qt::MiterJoin);
        p.setPen(pen);
        p.setBrush(Qt::NoBrush);
        const qreal fx = cx + (light % 2 ? 0.5 : 0.0), fy = cy + (light % 2 ? 0.5 : 0.0);
        if (ch >= 0x256d && ch <= 0x2570) {
            // Rounded corners: from the middle of one edge to the middle of the other.
            const qreal r = std::min(qreal(w) / 2, qreal(h) / 2);
            QPainterPath path;
            switch (ch) {
            case 0x256d:  // down and right
                path.moveTo(fx, y + h); path.lineTo(fx, fy + r); path.quadTo(fx, fy, fx + r, fy); path.lineTo(x + w, fy); break;
            case 0x256e:  // down and left
                path.moveTo(fx, y + h); path.lineTo(fx, fy + r); path.quadTo(fx, fy, fx - r, fy); path.lineTo(x, fy); break;
            case 0x256f:  // up and left
                path.moveTo(fx, y); path.lineTo(fx, fy - r); path.quadTo(fx, fy, fx - r, fy); path.lineTo(x, fy); break;
            default:  // up and right
                path.moveTo(fx, y); path.lineTo(fx, fy - r); path.quadTo(fx, fy, fx + r, fy); path.lineTo(x + w, fy); break;
            }
            p.drawPath(path);
        } else {
            if (ch == 0x2571 || ch == 0x2573) p.drawLine(QPointF(x + w, y), QPointF(x, y + h));
            if (ch == 0x2572 || ch == 0x2573) p.drawLine(QPointF(x, y), QPointF(x + w, y + h));
        }
        p.restore();
        return true;
    }
    if (ch >= 0x2580 && ch <= 0x259f) {
        auto rect = [&](qreal fx0, qreal fy0, qreal fx1, qreal fy1, int alpha = 255) {
            const int x0 = x + int(std::round(fx0 * w)), x1 = x + int(std::round(fx1 * w));
            const int y0 = y + int(std::round(fy0 * h)), y1 = y + int(std::round(fy1 * h));
            QColor c(255, 255, 255, alpha);
            p.fillRect(x0, y0, std::max(0, x1 - x0), std::max(0, y1 - y0), c);
        };
        const quint32 i = ch - 0x2580;
        if (i == 0) { rect(0, 0, 1, 0.5); return true; }
        if (i >= 1 && i <= 8) { rect(0, 1 - i / 8.0, 1, 1); return true; }
        if (i >= 9 && i <= 15) { rect(0, 0, (16 - i) / 8.0, 1); return true; }
        switch (ch) {
        case 0x2590: rect(0.5, 0, 1, 1); return true;
        case 0x2591: rect(0, 0, 1, 1, 64); return true;
        case 0x2592: rect(0, 0, 1, 1, 128); return true;
        case 0x2593: rect(0, 0, 1, 1, 192); return true;
        case 0x2594: rect(0, 0, 1, 1 / 8.0); return true;
        case 0x2595: rect(7 / 8.0, 0, 1, 1); return true;
        default: break;
        }
        // Quadrants 2596..259f: upper left, upper right, lower left, lower right bits.
        static const quint8 Q[10] = {4, 8, 1, 13, 9, 7, 11, 2, 6, 14};
        const quint8 q = Q[ch - 0x2596];
        if (q & 1) rect(0, 0, 0.5, 0.5);
        if (q & 2) rect(0.5, 0, 1, 0.5);
        if (q & 4) rect(0, 0.5, 0.5, 1);
        if (q & 8) rect(0.5, 0.5, 1, 1);
        return true;
    }
    if (ch >= 0xe0b0 && ch <= 0xe0bf) {
        p.save();
        p.setRenderHint(QPainter::Antialiasing, true);
        const qreal L = x, R = x + w, T = y, B = y + h, M = y + h / 2.0;
        QPainterPath path;
        bool fill = true;
        switch (ch) {
        case 0xe0b0: path.moveTo(L, T); path.lineTo(R, M); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0b1: path.moveTo(L, T); path.lineTo(R, M); path.lineTo(L, B); fill = false; break;
        case 0xe0b2: path.moveTo(R, T); path.lineTo(L, M); path.lineTo(R, B); path.closeSubpath(); break;
        case 0xe0b3: path.moveTo(R, T); path.lineTo(L, M); path.lineTo(R, B); fill = false; break;
        case 0xe0b4: path.moveTo(L, T); path.arcTo(QRectF(L - w, T, 2 * w, h), 90, -180); path.closeSubpath(); break;
        case 0xe0b5: path.moveTo(L, T); path.arcTo(QRectF(L - w, T, 2 * w, h), 90, -180); fill = false; break;
        case 0xe0b6: path.moveTo(R, T); path.arcTo(QRectF(L, T, 2 * w, h), 90, 180); path.closeSubpath(); break;
        case 0xe0b7: path.moveTo(R, T); path.arcTo(QRectF(L, T, 2 * w, h), 90, 180); fill = false; break;
        case 0xe0b8: path.moveTo(L, T); path.lineTo(R, B); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0b9: path.moveTo(L, T); path.lineTo(R, B); fill = false; break;
        case 0xe0ba: path.moveTo(R, T); path.lineTo(R, B); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0bb: path.moveTo(R, T); path.lineTo(L, B); fill = false; break;
        case 0xe0bc: path.moveTo(L, T); path.lineTo(R, T); path.lineTo(L, B); path.closeSubpath(); break;
        case 0xe0bd: path.moveTo(R, T); path.lineTo(L, B); fill = false; break;
        case 0xe0be: path.moveTo(L, T); path.lineTo(R, T); path.lineTo(R, B); path.closeSubpath(); break;
        default: path.moveTo(L, T); path.lineTo(R, B); fill = false; break;
        }
        if (fill)
            p.fillPath(path, white);
        else
            p.strokePath(path, QPen(white, light));
        p.restore();
        return true;
    }
    return false;
}
