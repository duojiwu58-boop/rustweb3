unit WalletTheme;
interface
uses Winapi.Windows, Winapi.Messages, System.Classes, System.SysUtils,
  System.Types, Vcl.Controls, Vcl.StdCtrls, Vcl.ExtCtrls, Vcl.Graphics, Vcl.Forms;
const
  Ink = $001B1917;
  Muted = $0085817C;
  Surface = $00F5F5F3;
  LineColor = $00EBEAE7;
  Lime = $0045EFCA;
type
  TWalletButton = class(TButton)
  private
    FPrimary: Boolean;
    FActionIcon: string;
    FBare: Boolean;
    procedure DrawItem(var M: TWMDrawItem); message CN_DRAWITEM;
  protected
    procedure CreateParams(var Params: TCreateParams); override;
    procedure SetButtonStyle(ADefault: Boolean); override;
  public
    constructor Create(AOwner: TComponent); override;
    property Primary: Boolean read FPrimary write FPrimary;
    property ActionIcon: string read FActionIcon write FActionIcon;
    property Bare: Boolean read FBare write FBare;
  end;
  TWalletEdit = class(TLabeledEdit)
  protected
    procedure CreateWnd; override;
    procedure Resize; override;
  end;
  TWalletPage = class(TPanel)
  public
    TabVisible: Boolean;
  end;
  TWalletDeck = class(TPanel)
  private
    FPages: array[0..4] of TWalletPage;
    FIndex, FCount: Integer;
    function GetPage(Index: Integer): TWalletPage;
    procedure SetIndex(Value: Integer);
    procedure SetPage(Value: TWalletPage);
    function GetActive: TWalletPage;
  public
    constructor Create(AOwner: TComponent); override;
    function AddPage: TWalletPage;
    property Pages[Index: Integer]: TWalletPage read GetPage;
    property ActivePageIndex: Integer read FIndex write SetIndex;
    property ActivePage: TWalletPage read GetActive write SetPage;
  end;
  TWalletMark = class(TGraphicControl)
  protected
    procedure Paint; override;
  end;
  TAssetRow = class(TCustomControl)
  private
    FSymbol, FAmount, FNetworkName: string;
    procedure SetAmount(const Value: string);
  protected
    procedure Paint; override;
  public
    constructor Create(AOwner: TComponent); override;
    property Symbol: string read FSymbol write FSymbol;
    property NetworkName: string read FNetworkName write FNetworkName;
    property Amount: string read FAmount write SetAmount;
    property OnClick;
  end;
function LabelText(Owner: TComponent; Host: TWinControl; const Text: string;
  X,Y,W,H,Size: Integer; Bold: Boolean = False; Tint: TColor = Ink): TLabel;
procedure FlatEdit(Edit: TLabeledEdit);
procedure FlatMemo(Memo: TMemo);
implementation
constructor TWalletButton.Create(AOwner: TComponent);
begin
  inherited; Cursor := crHandPoint; Font.Name := 'Microsoft YaHei UI'; Font.Size := 10;
  Font.Style := [fsBold]; DoubleBuffered := True;
end;
procedure TWalletButton.CreateParams(var Params: TCreateParams);
begin
  inherited; Params.Style := (Params.Style and not $000F) or BS_OWNERDRAW;
end;
procedure TWalletButton.SetButtonStyle(ADefault: Boolean);
begin Invalidate; end;
procedure TWalletButton.DrawItem(var M: TWMDrawItem);
var C: TCanvas; R, IconRect: TRect; Fill, TextColor: TColor; Radius, D: Integer;
begin
  C := TCanvas.Create;
  try
    C.Handle := M.DrawItemStruct.hDC; R := M.DrawItemStruct.rcItem;
    C.Brush.Color := clWhite; C.FillRect(R);
    Fill := Surface; TextColor := Ink;
    if FPrimary then begin Fill := Ink; TextColor := clWhite; end;
    if not Enabled then begin Fill := Surface; TextColor := $00B3B0AC; end
    else if (M.DrawItemStruct.itemState and ODS_SELECTED)<>0 then Fill := $00D6D4D0;
    if FBare then Fill := clWhite;
    C.Brush.Color := Fill; C.Pen.Color := Fill;
    Radius := MulDiv(24,CurrentPPI,96);
    if FActionIcon<>'' then
    begin
      D := MulDiv(52,CurrentPPI,96); IconRect := Rect((Width-D) div 2,0,(Width+D) div 2,D);
      C.Ellipse(IconRect); C.Font.Name := 'Segoe UI'; C.Font.Size := 23; C.Font.Color := Ink;
      C.Brush.Style := bsClear;
      DrawText(C.Handle,PChar(FActionIcon),Length(FActionIcon),IconRect,DT_CENTER or DT_VCENTER or DT_SINGLELINE);
      R.Top := D+MulDiv(6,CurrentPPI,96);
    end
    else C.RoundRect(R.Left,R.Top,R.Right,R.Bottom,Radius,Radius);
    C.Font.Assign(Font); C.Font.Color := TextColor; C.Brush.Style := bsClear;
    DrawText(C.Handle,PChar(Caption),Length(Caption),R,DT_CENTER or DT_VCENTER or DT_SINGLELINE);
    if Focused then begin InflateRect(R,-4,-4); C.DrawFocusRect(R); end;
  finally C.Free; end;
  M.Result := 1;
end;
procedure TWalletEdit.CreateWnd;
begin inherited; Resize; end;
procedure TWalletEdit.Resize;
begin
  inherited;
  if HandleAllocated then
  begin
    SetWindowRgn(Handle,CreateRoundRectRgn(0,0,Width+1,Height+1,MulDiv(12,CurrentPPI,96),MulDiv(12,CurrentPPI,96)),True);
    SendMessage(Handle,EM_SETMARGINS,EC_LEFTMARGIN or EC_RIGHTMARGIN,MakeLParam(MulDiv(12,CurrentPPI,96),MulDiv(12,CurrentPPI,96)));
  end;
end;
constructor TWalletDeck.Create(AOwner: TComponent);
begin inherited; BevelOuter := bvNone; Color := clWhite; ParentBackground := False; end;
function TWalletDeck.AddPage: TWalletPage;
begin
  if FCount>High(FPages) then raise Exception.Create('Too many wallet pages');
  Result := TWalletPage.Create(Self); Result.Parent := Self; Result.Align := alClient;
  Result.BevelOuter := bvNone; Result.Color := clWhite; Result.ParentBackground := False;
  Result.TabVisible := True; FPages[FCount] := Result; Inc(FCount); SetIndex(0);
end;
function TWalletDeck.GetPage(Index: Integer): TWalletPage;
begin Result := FPages[Index]; end;
function TWalletDeck.GetActive: TWalletPage;
begin Result := FPages[FIndex]; end;
procedure TWalletDeck.SetPage(Value: TWalletPage);
var I: Integer;
begin for I := 0 to FCount-1 do if FPages[I]=Value then SetIndex(I); end;
procedure TWalletDeck.SetIndex(Value: Integer);
var I: Integer;
begin
  FIndex := Value;
  for I := 0 to FCount-1 do FPages[I].Visible := I=Value;
end;
function LabelText(Owner: TComponent; Host: TWinControl; const Text: string;
  X,Y,W,H,Size: Integer; Bold: Boolean; Tint: TColor): TLabel;
begin
  Result := TLabel.Create(Owner); Result.Parent := Host; Result.AutoSize := False;
  Result.SetBounds(X,Y,W,H); Result.Caption := Text; Result.WordWrap := True;
  Result.Font.Name := 'Microsoft YaHei UI'; Result.Font.Size := Size; Result.Font.Color := Tint;
  if Bold then Result.Font.Style := [fsBold];
end;
procedure FlatEdit(Edit: TLabeledEdit);
begin
  Edit.BorderStyle := bsNone; Edit.Color := Surface; Edit.Font.Size := 12;
  Edit.EditLabel.Font.Size := 10; Edit.EditLabel.Font.Color := Muted;
end;
procedure FlatMemo(Memo: TMemo);
begin
  Memo.BorderStyle := bsNone; Memo.Color := Surface; Memo.Font.Size := 10;
  Memo.WordWrap := True; Memo.ScrollBars := ssVertical;
end;
procedure TWalletMark.Paint;
var Mark: HICON;
begin
  Mark := LoadImage(HInstance,'MAINICON',IMAGE_ICON,Width,Height,0);
  if Mark=0 then Exit;
  try
    DrawIconEx(Canvas.Handle,0,0,Mark,Width,Height,0,0,DI_NORMAL);
  finally DestroyIcon(Mark); end;
end;
constructor TAssetRow.Create(AOwner: TComponent);
begin inherited; DoubleBuffered := True; FAmount := '—'; FNetworkName := 'BNB Smart Chain'; end;
procedure TAssetRow.SetAmount(const Value: string);
begin if FAmount<>Value then begin FAmount := Value; Invalidate; end; end;
procedure TAssetRow.Paint;
var R: TRect; S, DecimalPos: Integer; Glyph, DisplayValue: string;
begin
  S := MulDiv(48,CurrentPPI,96);
  Canvas.Brush.Color := clWhite; Canvas.FillRect(ClientRect);
  Canvas.Pen.Color := LineColor; Canvas.MoveTo(0,Height-1); Canvas.LineTo(Width,Height-1);
  Canvas.Pen.Style := psClear;
  if FSymbol='USDT' then begin Canvas.Brush.Color := $008EAA26; Glyph := '₮'; end
  else if FSymbol='SOL' then begin Canvas.Brush.Color := $00814675; Glyph := '≋'; end
  else if FSymbol='TRX' then begin Canvas.Brush.Color := $004242DB; Glyph := 'T'; end
  else begin Canvas.Brush.Color := $000BB9F0; Glyph := '◆'; end;
  Canvas.Ellipse(0,(Height-S) div 2,S,(Height+S) div 2);
  Canvas.Font.Name := 'Segoe UI'; Canvas.Font.Size := 22; Canvas.Font.Color := clWhite;
  R := Rect(0,(Height-S) div 2,S,(Height+S) div 2); Canvas.Brush.Style := bsClear;
  DrawText(Canvas.Handle,PChar(Glyph),Length(Glyph),R,DT_CENTER or DT_VCENTER or DT_SINGLELINE);
  Canvas.Font.Name := 'Microsoft YaHei UI'; Canvas.Font.Size := 12; Canvas.Font.Style := [fsBold]; Canvas.Font.Color := Ink;
  Canvas.TextOut(S+MulDiv(14,CurrentPPI,96),MulDiv(20,CurrentPPI,96),FSymbol);
  Canvas.Font.Size := 9; Canvas.Font.Style := []; Canvas.Font.Color := Muted;
  Canvas.TextOut(S+MulDiv(14,CurrentPPI,96),MulDiv(46,CurrentPPI,96),FNetworkName);
  R := Rect(S+MulDiv(160,CurrentPPI,96),0,Width,Height);
  Canvas.Font.Size := 12; Canvas.Font.Style := [fsBold]; Canvas.Font.Color := Ink;
  DisplayValue := FAmount; DecimalPos := Pos('.',DisplayValue);
  if (DecimalPos>0) and (Length(DisplayValue)>DecimalPos+8) then DisplayValue := Copy(DisplayValue,1,DecimalPos+8)+'…';
  DrawText(Canvas.Handle,PChar(DisplayValue),Length(DisplayValue),R,DT_RIGHT or DT_VCENTER or DT_SINGLELINE or DT_END_ELLIPSIS);
  Canvas.Pen.Style := psSolid;
end;
end.
