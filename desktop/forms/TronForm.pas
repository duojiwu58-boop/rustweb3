unit TronForm;
interface
uses Winapi.Windows, System.SysUtils, System.Classes, System.JSON,
  Vcl.Forms, Vcl.Controls, Vcl.ExtCtrls, Vcl.StdCtrls, Vcl.ComCtrls, Vcl.Graphics,
  Vcl.Dialogs, WalletTheme, SecureWalletApi;
type
  TTronForm = class(TForm)
  private
    FPaused, FReadOnlyJob: Boolean;
    FHero: TLabel;
    FDetailAddress, FDetailInfo: TLabel;
    FDetailHistory: TMemo;
    FDetailCopy, FDetailRefresh, FDetailMore: TButton;
    FUsdtRow, FNativeRow: TAssetRow;
  private
    FApi: TSecureWalletApi;
    FProfile, FAddress, FQuote, FId, FSignature, FSummary, FState: string;
    FBusy, FActive, FPending, FJournalReady, FTest: Boolean;
    FGeneration: Integer;
    FExpiry, FNextBalance: UInt64;
    FWorker: TThread;
    FTimer: TTimer;
    FAddressLabel, FBalance, FNotice: TLabel;
    FRecipient, FAmount: TLabeledEdit;
    FAsset: TComboBox;
    FPreview, FHistory: TMemo;
    FPrepare, FSend, FRefresh, FQuery, FReplay, FCopy, FReload: TButton;
    FPages: TWalletDeck;
    function GetNavigationBusy: Boolean;
    procedure Navigate(Sender: TObject);
    procedure Changed(Sender: TObject);
    procedure Action(Sender: TObject);
    procedure Tick(Sender: TObject);
    procedure Closing(Sender: TObject; var CanClose: Boolean);
    procedure UpdateButtons;
    procedure Job(const Work: TFunc<UTF8String>; const Done: TProc<UTF8String>; ReadOnlyJob: Boolean = False);
    procedure LoadHistory;
    procedure ApplyHistory(const Value: UTF8String);
    procedure RefreshBalance;
    procedure Prepare;
    procedure Send(Replay: Boolean);
    procedure Query;
    function AssetName: string;
    function Button(Host: TWinControl; const Text: string; TagValue, X,Y,W: Integer): TButton;
  public
    constructor Create(AOwner: TComponent); override;
    destructor Destroy; override;
    procedure SelectPage(Index: Integer);
    procedure Pause(Value: Boolean);
    procedure OpenAccount(Api: TSecureWalletApi; const Profile, AccountName: string);
    procedure Lock;
    procedure RunSelfTest;
    procedure PublicPreview;
    property Busy: Boolean read FBusy;
    property NavigationBusy: Boolean read GetNavigationBusy;
  end;
implementation
uses Vcl.Clipbrd;
function SunAmount(const Text: string): string;
var Value: UInt64; Fraction: string;
begin
  Value := StrToUInt64(Text);
  Fraction := UIntToStr(Value mod 1000000);
  Result := UIntToStr(Value div 1000000)+'.'+StringOfChar('0',6-Length(Fraction))+Fraction;
end;
function NormalAmount(const Text: string): string;
begin
  Result := Trim(Text);
  while (Length(Result)>1) and (Result[1]='0') and (Result[2]<>'.') do Delete(Result,1,1);
  if Pos('.',Result)>0 then
  begin
    while (Length(Result)>0) and (Result[Length(Result)]='0') do Delete(Result,Length(Result),1);
    if (Length(Result)>0) and (Result[Length(Result)]='.') then Delete(Result,Length(Result),1);
  end;
end;
function Obj(const Value: UTF8String): TJSONObject;
var Parsed: TJSONValue;
begin
  Parsed := TJSONObject.ParseJSONValue(string(Value));
  if not (Parsed is TJSONObject) then begin Parsed.Free; raise Exception.Create('Tron 响应无效'); end;
  Result := TJSONObject(Parsed);
end;
function HexBytes(const Bytes: UTF8String): UTF8String;
const Digits: AnsiString = '0123456789abcdef';
var I, B: Integer;
begin
  SetLength(Result,Length(Bytes)*2);
  for I := 1 to Length(Bytes) do begin B := Ord(Bytes[I]); Result[I*2-1] := Digits[(B shr 4)+1]; Result[I*2] := Digits[(B and 15)+1]; end;
end;
function StateText(const State: string): string;
begin
  if State='finalized' then Result := '已最终确认'
  else if State='reverted' then Result := '链上执行失败'
  else if State='expired' then Result := '已过期，未查到上链记录'
  else Result := '结果待确认，请勿重复付款';
end;
function TTronForm.Button(Host: TWinControl; const Text: string; TagValue,X,Y,W: Integer): TButton;
begin
  Result := TWalletButton.Create(Self); Result.Parent := Host; Result.Caption := Text;
  Result.SetBounds(X,Y,W,42); Result.Tag := TagValue; Result.OnClick := Action;
end;
constructor TTronForm.Create(AOwner: TComponent);
var SendPage, HistoryPage, HomePage, ReceivePage, DetailPage: TWalletPage; B: TButton; L: TLabel;
  function Page(const Name: string): TWalletPage;
  begin
    Result := FPages.AddPage;
  end;
  procedure Nav(Host: TWinControl; const Text: string; Index,X,Y,W: Integer);
  begin
    B := Button(Host,Text,Index,X,Y,W); B.OnClick := Navigate;
  end;
begin
  inherited CreateNew(AOwner);
  Caption := 'Tron'; ClientWidth := 540; ClientHeight := 596;
  BorderStyle := bsNone; BorderIcons := []; Position := poDesigned;
  Font.Name := 'Microsoft YaHei UI'; Font.Size := 10; Color := clWhite; DoubleBuffered := True;
  FNotice := LabelText(Self,Self,'',32,0,476,52,9,False,Muted); FNotice.Align := alBottom; FNotice.WordWrap := True; FNotice.AlignWithMargins := True; FNotice.Margins.Left := 32; FNotice.Margins.Right := 32;
  FPages := TWalletDeck.Create(Self); FPages.Parent := Self; FPages.Align := alClient;
  SendPage := Page('发送'); HistoryPage := Page('交易记录'); HomePage := Page('钱包'); ReceivePage := Page('收款'); DetailPage := Page('USDT');
  Nav(HomePage,'发送',1,32,160,148); TWalletButton(B).ActionIcon := '↑'; B.Height := 76;
  Nav(HomePage,'接收',2,196,160,148); TWalletButton(B).ActionIcon := '↓'; B.Height := 76;
  Nav(HomePage,'记录',3,360,160,148); TWalletButton(B).ActionIcon := '↗'; B.Height := 76;
  LabelText(Self,HomePage,'我的资产',32,20,360,28,12,False,Muted);
  FHero := LabelText(Self,HomePage,'—',32,56,476,64,36,True);
  LabelText(Self,HomePage,'USDT · Tron 主网',32,124,360,24,10,False,Muted);
  LabelText(Self,HomePage,'币种',32,258,200,26,14,True);
  FUsdtRow := TAssetRow.Create(Self); FUsdtRow.Parent := HomePage; FUsdtRow.Symbol := 'USDT'; FUsdtRow.NetworkName := 'Tron'; FUsdtRow.Amount := '—';
  FUsdtRow.SetBounds(32,298,476,80); FUsdtRow.Tag := 3; FUsdtRow.Cursor := crHandPoint; FUsdtRow.OnClick := Navigate;
  FNativeRow := TAssetRow.Create(Self); FNativeRow.Parent := HomePage; FNativeRow.Symbol := 'TRX'; FNativeRow.NetworkName := 'Tron'; FNativeRow.Amount := '—';
  FNativeRow.SetBounds(32,388,476,80);
  FRefresh := Button(HomePage,'刷新余额',2,360,478,148);
  FBalance := LabelText(Self,HomePage,'',32,482,316,28,9,False,Muted);
  Nav(SendPage,'‹ 返回',0,32,12,88); LabelText(Self,SendPage,'发送',140,14,360,36,22,True);
  FAsset := TComboBox.Create(Self); FAsset.Parent := SendPage; FAsset.SetBounds(32,68,476,30);
  FAsset.Style := csDropDownList; FAsset.Items.Add('USDT · TRC20'); FAsset.ItemIndex := 0; FAsset.OnChange := Changed;
  FRecipient := TWalletEdit.Create(Self); FRecipient.Parent := SendPage; FRecipient.SetBounds(32,136,476,30); FlatEdit(FRecipient);
  FRecipient.EditLabel.Caption := '收款地址 · Tron'; FRecipient.OnChange := Changed;
  FAmount := TWalletEdit.Create(Self); FAmount.Parent := SendPage; FAmount.SetBounds(32,202,476,30); FlatEdit(FAmount);
  FAmount.EditLabel.Caption := '数量 · USDT'; FAmount.OnChange := Changed;
  FPreview := TMemo.Create(Self); FPreview.Parent := SendPage; FPreview.SetBounds(32,250,476,176);
  FPreview.ReadOnly := True; FPreview.ScrollBars := ssVertical; FlatMemo(FPreview);
  FPrepare := Button(SendPage,'核对转账',3,32,458,230);
  FSend := Button(SendPage,'确认并发送',4,278,458,230); TWalletButton(FSend).Primary := True;
  Nav(HistoryPage,'‹ 返回',3,32,12,88); LabelText(Self,HistoryPage,'本机转账',140,14,360,36,22,True);
  LabelText(Self,HistoryPage,'Tron · 最近两笔本机转账（待确认优先）',32,68,476,24,10,False,Muted);
  FHistory := TMemo.Create(Self); FHistory.Parent := HistoryPage; FHistory.SetBounds(32,108,476,274);
  FHistory.ReadOnly := True; FHistory.ScrollBars := ssVertical; FlatMemo(FHistory);
  FReload := Button(HistoryPage,'刷新本机记录',5,32,402,476);
  FQuery := Button(HistoryPage,'查询交易结果',6,32,458,230);
  FReplay := Button(HistoryPage,'重播原交易',7,278,458,230);
  Nav(ReceivePage,'‹ 返回',0,32,12,88); LabelText(Self,ReceivePage,'接收',140,14,360,36,22,True);
  L := LabelText(Self,ReceivePage,'接收 USDT / TRX',32,128,476,40,22,True); L.Alignment := taCenter;
  L := LabelText(Self,ReceivePage,'Tron 主网',32,184,476,28,12,False,Muted); L.Alignment := taCenter;
  FAddressLabel := LabelText(Self,ReceivePage,'',48,238,444,76,12,True); FAddressLabel.WordWrap := True; FAddressLabel.Alignment := taCenter;
  FCopy := Button(ReceivePage,'复制收款地址',1,32,350,476); TWalletButton(FCopy).Primary := True;
  L := LabelText(Self,ReceivePage,'请让转出方选择 Tron 网络',32,416,476,28,10,False,Muted); L.Alignment := taCenter;
  Nav(DetailPage,'‹ 返回',0,32,12,88);
  with TComboBox.Create(Self) do
  begin
    Parent := DetailPage; SetBounds(140,20,180,32); Style := csDropDownList;
    Items.Add('USDT'); ItemIndex := 0;
  end;
  Nav(DetailPage,'本机转账',4,382,16,118); B.Height := 40;
  LabelText(Self,DetailPage,'Tron · USDT 本机发送记录',40,76,460,26,10,False,Muted);
  Nav(DetailPage,'发送',1,40,106,222); B.Height := 76; TWalletButton(B).ActionIcon := '↑';
  Nav(DetailPage,'接收',2,278,106,222); B.Height := 76; TWalletButton(B).ActionIcon := '↓';
  LabelText(Self,DetailPage,'我的收款地址',40,190,340,22,10,False,Muted);
  FDetailAddress := LabelText(Self,DetailPage,'',40,216,340,48,10,True);
  FDetailCopy := Button(DetailPage,'复制地址',1,392,218,108); FDetailCopy.Height := 40;
  FDetailInfo := LabelText(Self,DetailPage,'仅最近两笔本机转账中的 USDT 记录，不含外部转入。',40,278,460,28,9,False,Muted);
  FDetailHistory := TMemo.Create(Self); FDetailHistory.Parent := DetailPage;
  FDetailHistory.SetBounds(40,310,460,216); FlatMemo(FDetailHistory);
  FDetailHistory.Color := clWhite; FDetailHistory.BorderStyle := bsNone;
  FDetailHistory.ReadOnly := True; FDetailHistory.ScrollBars := ssVertical;
  FDetailRefresh := Button(DetailPage,'刷新',5,40,542,144); FDetailRefresh.Height := 44;
  FDetailMore := Button(DetailPage,'更早记录',0,200,542,300); FDetailMore.Height := 44;
  FDetailMore.Enabled := False; FDetailMore.Hint := '暂不支持链上历史分页'; FDetailMore.ShowHint := True;
  FTimer := TTimer.Create(Self); FTimer.Interval := 500; FTimer.OnTimer := Tick;
  OnCloseQuery := Closing; SelectPage(0); UpdateButtons;
end;
function TTronForm.GetNavigationBusy: Boolean;
begin Result := FBusy and not FReadOnlyJob; end;
procedure TTronForm.SelectPage(Index: Integer);
begin
  if GetNavigationBusy then Exit;
  case Index of 0: FPages.ActivePageIndex := 2; 1: FPages.ActivePageIndex := 0;
    2: FPages.ActivePageIndex := 3; 3: FPages.ActivePageIndex := 4;
    4: FPages.ActivePageIndex := 1; end;
  FNotice.Visible := Index<>3;
end;
procedure TTronForm.Navigate(Sender: TObject);
begin
  if (Sender=FUsdtRow) or ((FPages.ActivePageIndex=4) and (TControl(Sender).Tag=1)) then begin FAsset.ItemIndex := 0; Changed(nil); end;
  SelectPage(TControl(Sender).Tag);
end;
procedure TTronForm.Pause(Value: Boolean);
begin
  FPaused := Value;
  if Value then begin Changed(nil); FRecipient.Clear; FAmount.Clear; end;
end;
destructor TTronForm.Destroy;
begin
  FTimer.Enabled := False;
  if FWorker<>nil then begin FWorker.WaitFor; TThread.RemoveQueuedEvents(FWorker); FWorker.Free; end;
  inherited;
end;
function TTronForm.AssetName: string;
begin Result := 'USDT'; end;
procedure TTronForm.UpdateButtons;
begin
  FDetailAddress.Caption := Copy(FAddress,1,22)+sLineBreak+Copy(FAddress,23,Length(FAddress));
  if FAddress='' then FDetailAddress.Caption := '';
  FDetailCopy.Enabled := FActive and (FAddress<>'') and not FTest;
  FDetailRefresh.Enabled := FActive and not FBusy and not FTest;
  FCopy.Enabled := FActive and (FAddress<>'') and not FTest;
  FRefresh.Enabled := FActive and not FBusy and not FTest;
  FAsset.Enabled := FActive and not FBusy; FRecipient.Enabled := FAsset.Enabled; FAmount.Enabled := FAsset.Enabled;
  FPrepare.Enabled := FActive and not FBusy and FJournalReady and not FPending and not FTest;
  FSend.Enabled := FPrepare.Enabled and (FQuote<>'') and (GetTickCount64<FExpiry);
  FReload.Enabled := FActive and not FBusy and not FTest;
  FQuery.Enabled := FReload.Enabled and (FSignature<>'');
  FReplay.Enabled := FQuery.Enabled and FPending;
end;
procedure TTronForm.Changed(Sender: TObject);
begin
  FQuote := ''; FExpiry := 0; FPreview.Clear;
  FAmount.EditLabel.Caption := '数量 · '+AssetName; UpdateButtons;
end;
procedure TTronForm.OpenAccount(Api: TSecureWalletApi; const Profile, AccountName: string);
begin
  FApi := Api; FProfile := Profile; FActive := True; FPending := True; FJournalReady := False;
  Caption := 'Tron · '+AccountName;
  FAddress := string(FApi.Request(31,[UTF8String(Profile)]));
  if (Length(FAddress)<>34) or (Copy(FAddress,1,1)<>'T') then raise Exception.Create('Tron 地址响应无效');
  FAddressLabel.Caption := FAddress; FNextBalance := 0;
  FNotice.Caption := 'USDT 使用此 Tron 地址；请先转入 TRX 激活账户并支付手续费。';
  LoadHistory;
end;
procedure TTronForm.Lock;
begin
  Inc(FGeneration); FActive := False; Changed(nil); FAddress := ''; FAddressLabel.Caption := '';
  FRecipient.Clear; FAmount.Clear; FHistory.Clear; FDetailHistory.Clear; FBalance.Caption := '';
  FHero.Caption := '—'; FUsdtRow.Amount := '—'; FNativeRow.Amount := '—'; FNotice.Caption := '钱包已锁定';
  UpdateButtons; if not FBusy and (Parent=nil) then Close;
end;
procedure TTronForm.Closing(Sender: TObject; var CanClose: Boolean);
begin
  CanClose := not FBusy;
  if not CanClose then FNotice.Caption := '正在处理请求，请稍候关闭。';
end;
procedure TTronForm.Job(const Work: TFunc<UTF8String>; const Done: TProc<UTF8String>; ReadOnlyJob: Boolean);
var Generation: Integer;
begin
  if FBusy or not FActive then Exit;
  FBusy := True; FReadOnlyJob := ReadOnlyJob; Generation := FGeneration; UpdateButtons;
  FWorker := TThread.CreateAnonymousThread(
    procedure
    var Value: UTF8String; Failure: string;
    begin
      try Value := Work(); except on E: Exception do Failure := E.Message; end;
      TThread.Queue(FWorker,
        procedure
        var Completed: TThread;
        begin
          Completed := FWorker; FWorker := nil; Completed.Free; FBusy := False; FReadOnlyJob := False;
          try
            if FActive and (Generation=FGeneration) then
              if Failure<>'' then FNotice.Caption := Failure + '；如已点击发送，请刷新本机记录并查询原交易。'
              else Done(Value);
          except on E: Exception do FNotice.Caption := E.Message; end;
          Wipe(Value); FNextBalance := GetTickCount64+15000; UpdateButtons;
          if not FActive and (Parent=nil) then Close;
        end);
    end);
  FWorker.FreeOnTerminate := False; FWorker.Start;
end;
procedure TTronForm.ApplyHistory(const Value: UTF8String);
var Data, Item: TJSONObject; Entries: TJSONArray; I: Integer; Parts: TArray<string>; Summary: string;
begin
  Data := Obj(Value);
  try
    if Data.GetValue<string>('address')<>FAddress then raise Exception.Create('Tron 账户记录不一致');
    Entries := Data.GetValue<TJSONArray>('entries'); FHistory.Clear; FDetailHistory.Clear; FId := ''; FSignature := ''; FSummary := ''; FState := '';
    FPending := False;
    for I := 0 to Entries.Count-1 do
    begin
      Item := Entries.Items[I] as TJSONObject; Summary := Item.GetValue<string>('summary'); Parts := Summary.Split([#10]);
      if (Length(Parts)<>11) or (Parts[1]<>'tron-mainnet') or (Parts[2]<>FAddress) then raise Exception.Create('Tron 记录格式不正确');
      if I=0 then begin FId := Item.GetValue<string>('id'); FSignature := Item.GetValue<string>('signature'); FSummary := Summary; FState := Item.GetValue<string>('state'); end;
      if not ((Item.GetValue<string>('state')='finalized') or (Item.GetValue<string>('state')='reverted') or (Item.GetValue<string>('state')='expired')) then FPending := True;
      if Parts[0]='USDT' then
      begin
        FDetailHistory.Lines.Add(Parts[4]+' USDT · '+StateText(Item.GetValue<string>('state')));
        FDetailHistory.Lines.Add('接收：'+Parts[3]);
        FDetailHistory.Lines.Add('交易：'+Item.GetValue<string>('signature'));
        FDetailHistory.Lines.Add('');
      end;
      FHistory.Lines.Add(Parts[4]+' '+Parts[0]+' — '+StateText(Item.GetValue<string>('state')));
      FHistory.Lines.Add('接收：'+Parts[3]); FHistory.Lines.Add('交易：'+Item.GetValue<string>('signature')); FHistory.Lines.Add('');
    end;
    if Entries.Count=0 then FHistory.Text := '暂无本机转账；外部转入请查看余额。';
    if FDetailHistory.Lines.Count=0 then FDetailHistory.Text := '最近本机记录中暂无 USDT 转账。';
    FJournalReady := True;
  finally Data.Free; end;
end;
procedure TTronForm.LoadHistory;
var Api: TSecureWalletApi; Profile: UTF8String; Epoch: UInt64;
begin
  Api := FApi; Epoch := Api.Epoch; Profile := UTF8String(FProfile); FJournalReady := False; FPending := True; Changed(nil);
  Job(function: UTF8String begin Result := Api.RequestAt(Epoch,34,[Profile]); end,
    procedure(Value: UTF8String) begin ApplyHistory(Value); end,True);
end;
procedure TTronForm.RefreshBalance;
var Api: TSecureWalletApi; Address: UTF8String;
begin
  Api := FApi; Address := UTF8String(FAddress);
  Job(function: UTF8String begin Result := Api.PublicRequest(18,[Address]); end,
    procedure(Value: UTF8String)
    var Data: TJSONObject;
    begin
      Data := Obj(Value);
      try
        if (Data.GetValue<string>('network')<>'tron-mainnet') or (Data.GetValue<string>('address')<>FAddress) or
          (Data.GetValue<string>('contract')<>'TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t') or (Data.GetValue<Integer>('usdt_decimals')<>6) then raise Exception.Create('Tron 余额身份不一致');
        FHero.Caption := Data.GetValue<string>('usdt');
        FUsdtRow.Amount := FHero.Caption; FNativeRow.Amount := Data.GetValue<string>('trx');
        FBalance.Caption := '已更新 · '+FormatDateTime('hh:nn:ss',Now);
      finally Data.Free; end;
    end,True);
end;
procedure TTronForm.Prepare;
var Api: TSecureWalletApi; Address, Recipient, AssetAmount: UTF8String; Asset, AmountText: string;
begin
  if not FPrepare.Enabled then Exit;
  Api := FApi; Address := UTF8String(FAddress); Recipient := UTF8String(Trim(FRecipient.Text));
  Asset := AssetName; AmountText := Trim(FAmount.Text); AssetAmount := UTF8String(Asset+#10+AmountText); Changed(nil);
  Job(function: UTF8String begin Result := Api.PublicRequest(19,[Address,Recipient,AssetAmount]); end,
    procedure(Value: UTF8String)
    var Data: TJSONObject; Parts: TArray<string>; Summary: string;
    begin
      Data := Obj(Value);
      try
        Summary := Data.GetValue<string>('summary'); Parts := Summary.Split([#10]);
        if (Length(Parts)<>11) or (Parts[0]<>Asset) or (Parts[1]<>'tron-mainnet') or
          (Parts[2]<>string(Address)) or (Parts[3]<>string(Recipient)) or
          (NormalAmount(Parts[4])<>NormalAmount(AmountText)) then raise Exception.Create('Tron 核对信息不一致');
        FQuote := Summary; FExpiry := GetTickCount64+20000;
        FPreview.Text := Parts[4]+' '+Parts[0]+sLineBreak+'Tron 主网 → '+Parts[3]+sLineBreak+
          '能量费用上限：'+SunAmount(Parts[9])+' TRX'+sLineBreak+
          '带宽费用预留：'+SunAmount(Parts[10])+' TRX'+sLineBreak+'请在 20 秒内确认，过期后重新核对。';
        FNotice.Caption := '请核对完整收款地址、金额和 TRX 费用。';
      finally Data.Free; end;
    end);
end;
procedure TTronForm.Send(Replay: Boolean);
var Api: TSecureWalletApi; Profile, Summary, Id, Signature: UTF8String; Epoch: UInt64;
begin
  if Replay then begin if not FReplay.Enabled then Exit; end
  else if not FSend.Enabled then Exit;
  if Replay then
  begin if MessageDlg('仅重播原交易，不创建新付款。继续？',mtConfirmation,[mbYes,mbNo],0,mbNo)<>mrYes then Exit; end
  else if MessageDlg(FPreview.Text+sLineBreak+'确认发送？',mtConfirmation,[mbYes,mbNo],0,mbNo)<>mrYes then Exit;
  if not FActive or (not Replay and (GetTickCount64>=FExpiry)) then begin Changed(nil); Exit; end;
  Api := FApi; Epoch := Api.Epoch; Profile := UTF8String(FProfile);
  if Replay then begin Summary := UTF8String(FSummary); Id := UTF8String(FId); Signature := UTF8String(FSignature); end
  else Summary := UTF8String(FQuote);
  FPending := True; FJournalReady := False; Changed(nil); FPages.ActivePageIndex := 1;
  Job(function: UTF8String
    var Signed, Raw, BroadcastReply: UTF8String; Lines: TArray<string>; Data: TJSONObject;
    begin
      try
        if not Replay then
        begin
          Signed := Api.RequestAt(Epoch,32,[Profile,Summary]); Lines := string(Signed).Split([#10]);
          if Length(Lines)<>13 then raise Exception.Create('Tron 签名响应无效');
          Id := UTF8String(Lines[0]); Signature := UTF8String(Lines[1]);
          if Copy(string(Signed),Length(Lines[0])+Length(Lines[1])+3,MaxInt)<>string(Summary) then raise Exception.Create('Tron 签名摘要不一致');
        end;
        Raw := Api.RequestAt(Epoch,33,[Profile,Id]);
        BroadcastReply := Api.PublicRequest(20,[HexBytes(Raw),Signature,Summary]);
        Data := Obj(BroadcastReply);
        try if Data.GetValue<string>('signature')<>string(Signature) then raise Exception.Create('Tron 广播返回不一致'); finally Data.Free; end;
        Result := Api.RequestAt(Epoch,34,[Profile]);
      finally Wipe(Signed); Wipe(Raw); Wipe(BroadcastReply); end;
    end,
    procedure(Value: UTF8String) begin ApplyHistory(Value); FNotice.Caption := '已提交，尚未最终确认。请点击查询交易结果。'; end);
end;
procedure TTronForm.Query;
var Api: TSecureWalletApi; Profile, Id, Signature, Summary: UTF8String; Epoch: UInt64;
begin
  if not FQuery.Enabled then Exit;
  Api := FApi; Epoch := Api.Epoch; Profile := UTF8String(FProfile); Id := UTF8String(FId); Signature := UTF8String(FSignature); Summary := UTF8String(FSummary);
  Job(function: UTF8String
    var Reply: UTF8String; Data: TJSONObject; State: string;
    begin
      Reply := Api.PublicRequest(21,[Signature,Summary]); Data := Obj(Reply);
      try
        if (Data.GetValue<string>('network')<>'tron-mainnet') or (Data.GetValue<string>('signature')<>string(Signature)) then raise Exception.Create('Tron 交易结果不一致');
        State := Data.GetValue<string>('state');
        Api.RequestAt(Epoch,35,[Profile,Id,Signature,UTF8String(State)]);
        Result := Api.RequestAt(Epoch,34,[Profile]);
      finally Data.Free; end;
    end,
    procedure(Value: UTF8String) begin ApplyHistory(Value); FNotice.Caption := StateText(FState); end);
end;
procedure TTronForm.Tick(Sender: TObject);
begin
  UpdateButtons;
  if not FActive or FPaused or FBusy or FTest or not Visible then Exit;
  if FJournalReady and (FQuote='') and (GetTickCount64>=FNextBalance) then RefreshBalance;
end;
procedure TTronForm.Action(Sender: TObject);
begin
  try
    case TButton(Sender).Tag of
      1: if FCopy.Enabled then begin Clipboard.AsText := FAddress; FDetailCopy.Caption := '已复制'; end;
      2: if FRefresh.Enabled then RefreshBalance;
      3: Prepare;
      4: Send(False);
      5: if FReload.Enabled then LoadHistory;
      6: Query;
      7: Send(True);
    end;
  except on E: Exception do FNotice.Caption := E.Message; end;
end;
procedure TTronForm.PublicPreview;
begin
  FTest := True; FActive := True; FAddress := 'PUBLIC PREVIEW - DO NOT FUND';
  FAddressLabel.Caption := FAddress; FBalance.Caption := 'TRX 1.234567890   /   USDT 12.345678';
  FHero.Caption := '12.345678'; FUsdtRow.Amount := FHero.Caption; FNativeRow.Amount := '1.234567';
  FBalance.Caption := '示例余额';
  FNotice.Caption := '公开界面预览 · 示例余额 · 不可收款或发送';
  FHistory.Text := '暂无本机转账；外部转入请查看余额。';
  FDetailHistory.Text := '最近本机记录中暂无 USDT 转账。';
  UpdateButtons;
end;
procedure TTronForm.RunSelfTest;
var Ppi: Integer;
begin
  PublicPreview;
  FAsset.ItemIndex := 0; Changed(FAsset);
  if FAmount.EditLabel.Caption<>'数量 · USDT' then raise Exception.Create('Tron asset switch failed');
  if FSend.Enabled or FCopy.Enabled then raise Exception.Create('Tron public preview enabled money actions');
  for Ppi in [96,144,96] do
  begin
    ScaleForPPI(Ppi); ClientWidth := MulDiv(540,Ppi,96); ClientHeight := MulDiv(596,Ppi,96);
    Navigate(FUsdtRow);
    if FPages.ActivePageIndex<>4 then raise Exception.Create('USDT row did not open asset details');
    if FNotice.Visible or (FDetailMore.Top+FDetailMore.Height>FPages.Pages[4].ClientHeight) then
      raise Exception.Create('Asset details layout clipped');
    SelectPage(4);
    if FPages.ActivePageIndex<>1 then raise Exception.Create('Local transfers route failed');
    SelectPage(0);
    FPages.ActivePageIndex := 0;
    if FSend.Top+FSend.Height>FPages.Pages[0].ClientHeight then raise Exception.Create('Tron send controls clipped');
    FPages.ActivePageIndex := 1;
    if FReplay.Top+FReplay.Height>FPages.Pages[1].ClientHeight then raise Exception.Create('Tron history controls clipped');
  end;
  FQuote := 'TEST PREVIEW'; FRecipient.Text := 'TEST RECIPIENT'; FAmount.Text := '1';
  FBusy := True; FReadOnlyJob := True;
  if GetNavigationBusy then raise Exception.Create('Read-only refresh blocked network navigation');
  SelectPage(2);
  if FPages.ActivePageIndex<>3 then raise Exception.Create('Read-only refresh blocked receive page');
  FReadOnlyJob := False;
  if not GetNavigationBusy then raise Exception.Create('Transfer operation allowed network navigation');
  FBusy := False;
  Pause(True);
  if (FQuote<>'') or (FRecipient.Text<>'') or (FAmount.Text<>'') or not FPaused then raise Exception.Create('Chain switch retained transfer draft');
  Pause(False);
  Lock;
  if FAddressLabel.Caption<>'' then raise Exception.Create('Tron lock did not clear address');
end;
end.
