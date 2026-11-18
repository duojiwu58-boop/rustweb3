unit SecretEdit;

interface
uses Winapi.Windows, Winapi.Messages, System.Classes, System.SysUtils,
  System.Types, System.Math, Vcl.Controls, Vcl.StdCtrls, Vcl.Graphics;

// Secrets never enter a native EDIT, VCL Text/Caption, undo history or a
// managed UnicodeString. The fixed UTF-16 allocation is wiped in place.
type
  TSecretEdit = class(TCustomControl)
  private
    FData: array[0..256] of WideChar;
    FCount, FCursor, FAnchor, FScroll: Integer;
    FReadOnly, FMultiline, FDragging, FMnemonicGrid: Boolean;
    FPasswordChar: Char;
    FLabel: TLabel;
    FPoints: array[0..256] of TPoint;
    FLineHeight: Integer;
    procedure LayoutText;
    procedure PaintMnemonicGrid;
    procedure Changed;
    procedure DeleteSelection;
    procedure InsertChars(Data: PWideChar; Count: Integer);
    procedure Paste;
    function PreviousChar(Index: Integer): Integer;
    function NextChar(Index: Integer): Integer;
    function Hit(X, Y: Integer): Integer;
    procedure WMGetDlgCode(var M: TMessage); message WM_GETDLGCODE;
    procedure WMSetFocus(var M: TMessage); message WM_SETFOCUS;
    procedure WMKillFocus(var M: TMessage); message WM_KILLFOCUS;
    procedure WMPaste(var M: TMessage); message WM_PASTE;
  protected
    procedure Paint; override;
    procedure KeyDown(var Key: Word; Shift: TShiftState); override;
    procedure KeyPress(var Key: Char); override;
    procedure MouseDown(Button: TMouseButton; Shift: TShiftState; X,Y: Integer); override;
    procedure MouseMove(Shift: TShiftState; X,Y: Integer); override;
    procedure MouseUp(Button: TMouseButton; Shift: TShiftState; X,Y: Integer); override;
  public
    constructor Create(AOwner: TComponent); override;
    destructor Destroy; override;
    procedure SetBounds(ALeft, ATop, AWidth, AHeight: Integer); override;
    procedure Clear;
    procedure SetSecret(const Value: UTF8String);
    procedure ReadSecret(var Value: UTF8String; TrimEdges: Boolean = False);
    function SameSecret(Other: TSecretEdit): Boolean;
    function IsEmpty: Boolean;
    procedure CheckErased;
    procedure RunBufferSelfTest;
    property Font;
    property EditLabel: TLabel read FLabel;
    property PasswordChar: Char read FPasswordChar write FPasswordChar;
    property ReadOnly: Boolean read FReadOnly write FReadOnly;
    property Multiline: Boolean read FMultiline write FMultiline;
    property MnemonicGrid: Boolean read FMnemonicGrid write FMnemonicGrid;
  end;

implementation
uses SecureWalletApi;

constructor TSecretEdit.Create(AOwner: TComponent);
begin
  inherited;
  TabStop := True; Cursor := crIBeam;
  // Avoid a VCL offscreen bitmap retaining rendered mnemonic text.
  DoubleBuffered := False; ParentDoubleBuffered := False;
  ControlStyle := ControlStyle + [csOpaque];
  Color := $00F5F5F3; Font.Name := 'Microsoft YaHei UI'; Font.Size := 10;
  FLabel := TLabel.Create(Self); FLabel.Font.Size := 10; FLabel.Font.Color := $0085817C;
end;

destructor TSecretEdit.Destroy;
begin
  Clear;
  inherited;
end;

procedure TSecretEdit.SetBounds(ALeft, ATop, AWidth, AHeight: Integer);
begin
  inherited;
  if Assigned(FLabel) then
  begin
    FLabel.Parent := Parent;
    FLabel.SetBounds(ALeft, ATop-MulDiv(24,CurrentPPI,96), AWidth, MulDiv(22,CurrentPPI,96));
  end;
end;

procedure TSecretEdit.Clear;
begin
  FillChar(FData, SizeOf(FData), 0);
  FCount := 0; FCursor := 0; FAnchor := 0; FScroll := 0;
  if HandleAllocated then Changed;
end;

procedure TSecretEdit.CheckErased;
var I: Integer;
begin
  for I := 0 to High(FData) do
    if FData[I]<>#0 then raise Exception.Create('Sensitive control buffer not erased');
  if (FCount<>0) or (FCursor<>0) or (FAnchor<>0) then
    raise Exception.Create('Sensitive control state not reset');
end;

procedure TSecretEdit.RunBufferSelfTest;
var Value: UTF8String; I: Integer; K: Word; Ch: Char;
    Sample: array[0..2] of WideChar;
begin
  // Public fixtures only. Exercise actual key/edit paths, not just Clear.
  try
    FReadOnly := False;
    SetSecret('PUBLIC TEST ONLY');
    FAnchor := 0; FCursor := FCount; Ch := 'X'; KeyPress(Ch);
    ReadSecret(Value);
    if Value<>'X' then raise Exception.Create('Sensitive selection replacement failed');
    for I := 1 to High(FData) do
      if FData[I]<>#0 then raise Exception.Create('Deleted sensitive tail retained');
    K := VK_BACK; KeyDown(K,[]); CheckErased;
    SetSecret('  PUBLIC TEST  '); ReadSecret(Value,True);
    if Value<>'PUBLIC TEST' then raise Exception.Create('Sensitive trim failed');
    Clear; CheckErased;
    // UTF-16 -> UTF-8 -> UTF-16, including a surrogate pair.
    Sample[0] := #$4E2D; Sample[1] := #$D83D; Sample[2] := #$DE00;
    InsertChars(@Sample[0],Length(Sample)); ReadSecret(Value);
    if Length(Value)<>7 then raise Exception.Create('Sensitive Unicode conversion failed');
    SetSecret(Value);
    if (FCount<>3) or not CompareMem(@FData[0],@Sample[0],SizeOf(Sample)) then
      raise Exception.Create('Sensitive Unicode roundtrip failed');
    K := VK_BACK; KeyDown(K,[]);
    if (FCount<>1) or (FData[1]<>#0) or (FData[2]<>#0) then
      raise Exception.Create('Sensitive surrogate deletion failed');
    // Native window text must stay empty while a secret is present.
    if GetWindowTextLengthW(Handle)<>0 then raise Exception.Create('Secret leaked to window text');
    Clear; CheckErased;
    Perform(WM_CHAR,Ord('Q'),0); ReadSecret(Value);
    if Value<>'Q' then raise Exception.Create('Sensitive WM_CHAR input failed');
    Clear; CheckErased;
    for I := 1 to 256 do begin Ch := 'x'; KeyPress(Ch); end;
    Ch := 'y'; KeyPress(Ch);
    if FCount<>256 then raise Exception.Create('Sensitive length limit failed');
    FAnchor := 0; FCursor := FCount; K := VK_DELETE; KeyDown(K,[]); CheckErased;
  finally
    Wipe(Value); FillChar(Sample,SizeOf(Sample),0); Clear;
  end;
end;

function TSecretEdit.IsEmpty: Boolean;
begin Result := FCount=0; end;

function TSecretEdit.SameSecret(Other: TSecretEdit): Boolean;
begin Result := (FCount=Other.FCount) and CompareMem(@FData[0],@Other.FData[0],SizeOf(FData)); end;

procedure TSecretEdit.SetSecret(const Value: UTF8String);
var N: Integer;
begin
  Clear;
  if Value='' then Exit;
  N := MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, PAnsiChar(Value), Length(Value), @FData[0], 256);
  if N=0 then begin Clear; raise Exception.Create('Invalid sensitive text'); end;
  FCount := N; FCursor := N; FAnchor := N; Changed;
end;

procedure TSecretEdit.ReadSecret(var Value: UTF8String; TrimEdges: Boolean);
var First, Last, N: Integer;
begin
  Wipe(Value);
  First := 0; Last := FCount;
  if TrimEdges then
  begin
    while (First<Last) and (FData[First]<=#32) do Inc(First);
    while (Last>First) and (FData[Last-1]<=#32) do Dec(Last);
  end;
  if First=Last then Exit;
  N := WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, @FData[First], Last-First, nil, 0, nil, nil);
  if N=0 then raise Exception.Create('Invalid sensitive text');
  SetLength(Value,N);
  if WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, @FData[First], Last-First, PAnsiChar(Value), N, nil, nil)<>N then
  begin Wipe(Value); raise Exception.Create('Sensitive text conversion failed'); end;
end;

procedure TSecretEdit.DeleteSelection;
var First, Last, Remaining: Integer;
begin
  First := Min(FCursor,FAnchor); Last := Max(FCursor,FAnchor);
  Remaining := FCount-Last;
  if Remaining>0 then Move(FData[Last],FData[First],Remaining*SizeOf(WideChar));
  Dec(FCount,Last-First);
  FillChar(FData[FCount],(Length(FData)-FCount)*SizeOf(WideChar),0);
  FCursor := First; FAnchor := First;
end;

procedure TSecretEdit.InsertChars(Data: PWideChar; Count: Integer);
var I: Integer;
begin
  if FReadOnly or (Count<=0) then Exit;
  // Reject oversize paste atomically; never silently truncate a password.
  if Count>256-(FCount-Abs(FCursor-FAnchor)) then Exit;
  for I := 0 to Count-1 do
    if (Data[I]<#32) and not (FMultiline and ((Data[I]=#9) or (Data[I]=#10) or (Data[I]=#13))) then Exit;
  DeleteSelection;
  if FCursor<FCount then Move(FData[FCursor],FData[FCursor+Count],(FCount-FCursor)*SizeOf(WideChar));
  Move(Data^,FData[FCursor],Count*SizeOf(WideChar));
  Inc(FCount,Count); Inc(FCursor,Count); FAnchor := FCursor; Changed;
end;

procedure TSecretEdit.Paste;
var Data: PWideChar; Memory: THandle; Capacity: NativeUInt; N: Integer;
begin
  if FReadOnly or not OpenClipboard(Handle) then Exit;
  try
    Memory := GetClipboardData(CF_UNICODETEXT);
    if Memory=0 then Exit;
    Capacity := GlobalSize(Memory) div SizeOf(WideChar);
    Data := GlobalLock(Memory);
    if Data=nil then Exit;
    try
      N := 0;
      while (NativeUInt(N)<Capacity) and (N<=256) and (Data[N]<>#0) do Inc(N);
      if (N<=256) and (NativeUInt(N)<Capacity) then InsertChars(Data,N);
    finally GlobalUnlock(Memory); end;
  finally CloseClipboard; end;
end;

procedure TSecretEdit.LayoutText;
var I, X, Y, Padding, W: Integer; Size: TSize; C: WideChar;
begin
  Canvas.Font.Assign(Font); Padding := MulDiv(10,CurrentPPI,96);
  GetTextExtentPoint32W(Canvas.Handle,'Mg',2,Size); FLineHeight := Size.cy+2;
  X := Padding; Y := Padding;
  if not FMultiline then Y := Max(0,(Height-FLineHeight) div 2);
  for I := 0 to FCount-1 do
  begin
    C := FData[I]; if FPasswordChar<>#0 then C := FPasswordChar;
    GetTextExtentPoint32W(Canvas.Handle,@C,1,Size); W := Size.cx;
    if FMultiline and (X>Padding) and (X+W>Width-Padding) then begin X := Padding; Inc(Y,FLineHeight); end;
    FPoints[I] := Point(X,Y);
    if FMultiline and (C=#10) then begin X := Padding; Inc(Y,FLineHeight); end
    else if C<>#13 then Inc(X,W);
  end;
  FPoints[FCount] := Point(X,Y); C := #0;
end;

procedure TSecretEdit.Changed;
begin
  if not HandleAllocated then Exit;
  if FMnemonicGrid then begin FScroll := 0; Invalidate; Exit; end;
  LayoutText;
  if FMultiline then
  begin
    if FPoints[FCursor].Y-FScroll<0 then FScroll := FPoints[FCursor].Y;
    if FPoints[FCursor].Y-FScroll+FLineHeight>Height then FScroll := FPoints[FCursor].Y+FLineHeight-Height;
  end else
  begin
    if FPoints[FCursor].X-FScroll<10 then FScroll := Max(0,FPoints[FCursor].X-10);
    if FPoints[FCursor].X-FScroll>Width-10 then FScroll := FPoints[FCursor].X-Width+10;
  end;
  Invalidate;
end;

procedure TSecretEdit.PaintMnemonicGrid;
var I, First, N, X, Y, CellWidth, RowHeight, Padding, WordCount: Integer;
    NumberText: string; R: TRect;
begin
  Canvas.Font.Assign(Font);
  Canvas.Brush.Color := Color; Canvas.FillRect(ClientRect);
  Padding := MulDiv(10,CurrentPPI,96);
  WordCount := 0;
  for I := 0 to FCount-1 do
    if (FData[I]>#32) and ((I=0) or (FData[I-1]<=#32)) then Inc(WordCount);
  CellWidth := Width div 2; RowHeight := Height div Max(1,(WordCount+1) div 2);
  I := 0; N := 0;
  while (I<FCount) and (N<24) do
  begin
    while (I<FCount) and (FData[I]<=#32) do Inc(I);
    if I=FCount then Break;
    First := I;
    while (I<FCount) and (FData[I]>#32) do Inc(I);
    X := (N mod 2)*CellWidth; Y := (N div 2)*RowHeight;
    NumberText := IntToStr(N+1)+'.';
    Canvas.Font.Color := $0085817C;
    Canvas.TextOut(X+Padding,Y+(RowHeight-Canvas.TextHeight('Mg')) div 2,NumberText);
    R := Rect(X+MulDiv(36,CurrentPPI,96),Y,X+CellWidth-Padding,Y+RowHeight);
    SetTextColor(Canvas.Handle,ColorToRGB(Font.Color));
    SetBkMode(Canvas.Handle,TRANSPARENT);
    // Draw directly from the wipeable buffer; numbering never alters the mnemonic.
    DrawTextW(Canvas.Handle,@FData[First],I-First,R,DT_LEFT or DT_VCENTER or DT_SINGLELINE or DT_NOPREFIX);
    Inc(N);
  end;
end;

procedure TSecretEdit.Paint;
var I, X, Y, First, Last: Integer; R: TRect; C: WideChar; Size: TSize;
begin
  if FMnemonicGrid then begin PaintMnemonicGrid; Exit; end;
  LayoutText; Canvas.Brush.Color := Color; Canvas.FillRect(ClientRect);
  First := Min(FCursor,FAnchor); Last := Max(FCursor,FAnchor);
  for I := 0 to FCount-1 do
  begin
    C := FData[I]; if FPasswordChar<>#0 then C := FPasswordChar;
    if (C=#10) or (C=#13) then Continue;
    X := FPoints[I].X; Y := FPoints[I].Y;
    if FMultiline then Dec(Y,FScroll) else Dec(X,FScroll);
    GetTextExtentPoint32W(Canvas.Handle,@C,1,Size);
    R := Rect(X,Y,X+Size.cx,Y+FLineHeight);
    if Focused and (I>=First) and (I<Last) then
    begin SetBkColor(Canvas.Handle,ColorToRGB(clHighlight)); SetTextColor(Canvas.Handle,ColorToRGB(clHighlightText)); end
    else begin SetBkColor(Canvas.Handle,ColorToRGB(Color)); SetTextColor(Canvas.Handle,ColorToRGB(Font.Color)); end;
    ExtTextOutW(Canvas.Handle,X,Y,ETO_OPAQUE or ETO_CLIPPED,@R,@C,1,nil);
  end;
  C := #0;
  if Focused then
  begin
    X := FPoints[FCursor].X; Y := FPoints[FCursor].Y;
    if FMultiline then Dec(Y,FScroll) else Dec(X,FScroll);
    SetCaretPos(X,Y);
    R := ClientRect; InflateRect(R,-1,-1); Canvas.DrawFocusRect(R);
  end;
end;

function TSecretEdit.Hit(X,Y: Integer): Integer;
var I, Distance, Best: Integer;
begin
  LayoutText;
  if FMultiline then Inc(Y,FScroll) else Inc(X,FScroll);
  Result := 0; Best := MaxInt;
  for I := 0 to FCount do
  begin
    if (I>0) and (I<FCount) and (Ord(FData[I])>=$DC00) and (Ord(FData[I])<=$DFFF)
      and (Ord(FData[I-1])>=$D800) and (Ord(FData[I-1])<=$DBFF) then Continue;
    Distance := Abs(FPoints[I].Y+FLineHeight div 2-Y)*1024+Abs(FPoints[I].X-X);
    if Distance<Best then begin Best := Distance; Result := I; end;
  end;
end;

function TSecretEdit.PreviousChar(Index: Integer): Integer;
begin
  Result := Max(0,Index-1);
  if (Result>0) and (Ord(FData[Result])>=$DC00) and (Ord(FData[Result])<=$DFFF)
    and (Ord(FData[Result-1])>=$D800) and (Ord(FData[Result-1])<=$DBFF) then Dec(Result);
end;

function TSecretEdit.NextChar(Index: Integer): Integer;
begin
  Result := Min(FCount,Index+1);
  if (Index<FCount) and (Result<FCount) and (Ord(FData[Index])>=$D800) and (Ord(FData[Index])<=$DBFF)
    and (Ord(FData[Result])>=$DC00) and (Ord(FData[Result])<=$DFFF) then Inc(Result);
end;

procedure TSecretEdit.MouseDown(Button: TMouseButton; Shift: TShiftState; X,Y: Integer);
begin
  inherited;
  if FMnemonicGrid then Exit;
  if Button<>mbLeft then Exit;
  SetFocus; FCursor := Hit(X,Y); if not (ssShift in Shift) then FAnchor := FCursor;
  FDragging := True; MouseCapture := True; Changed;
end;
procedure TSecretEdit.MouseMove(Shift: TShiftState; X,Y: Integer);
begin inherited; if FDragging then begin FCursor := Hit(X,Y); Changed; end; end;
procedure TSecretEdit.MouseUp(Button: TMouseButton; Shift: TShiftState; X,Y: Integer);
begin inherited; FDragging := False; MouseCapture := False; end;

procedure TSecretEdit.KeyDown(var Key: Word; Shift: TShiftState);
begin
  inherited;
  if ssCtrl in Shift then
  begin
    case Key of
      Ord('A'): begin FAnchor := 0; FCursor := FCount; Changed; end;
      Ord('V'): Paste;
    end;
    // No copy/cut/undo buffer for secrets.
    Key := 0; Exit;
  end;
  if (Key=VK_INSERT) and (ssShift in Shift) then begin Paste; Key := 0; Exit; end;
  case Key of
    VK_LEFT: FCursor := PreviousChar(FCursor);
    VK_RIGHT: FCursor := NextChar(FCursor);
    VK_HOME: FCursor := 0;
    VK_END: FCursor := FCount;
    VK_UP: begin LayoutText; FCursor := Hit(FPoints[FCursor].X,FPoints[FCursor].Y-FScroll-FLineHeight div 2); end;
    VK_DOWN: begin LayoutText; FCursor := Hit(FPoints[FCursor].X,FPoints[FCursor].Y-FScroll+FLineHeight*3 div 2); end;
    VK_DELETE, VK_BACK:
      begin
        if not FReadOnly then
        begin
          if FCursor=FAnchor then
            if Key=VK_BACK then FAnchor := PreviousChar(FCursor) else FAnchor := NextChar(FCursor);
          DeleteSelection;
        end;
      end;
    else Exit;
  end;
  if not (ssShift in Shift) then FAnchor := FCursor;
  Changed; Key := 0;
end;

procedure TSecretEdit.KeyPress(var Key: Char);
begin
  if Key>=#32 then InsertChars(@Key,1)
  else if (Key=#13) and FMultiline then begin Key := #10; InsertChars(@Key,1); end;
  Key := #0;
end;
procedure TSecretEdit.WMGetDlgCode(var M: TMessage);
begin
  M.Result := DLGC_WANTARROWS or DLGC_WANTCHARS;
  if FMultiline then M.Result := M.Result or DLGC_WANTALLKEYS;
  // Tab retains ordinary form navigation, including multiline fields.
  if M.WParam=VK_TAB then M.Result := DLGC_WANTCHARS;
end;
procedure TSecretEdit.WMSetFocus(var M: TMessage);
begin inherited; LayoutText; CreateCaret(Handle,0,1,FLineHeight); ShowCaret(Handle); Changed; end;
procedure TSecretEdit.WMKillFocus(var M: TMessage);
begin DestroyCaret; FDragging := False; inherited; Invalidate; end;
procedure TSecretEdit.WMPaste(var M: TMessage);
begin Paste; M.Result := 0; end;

end.
