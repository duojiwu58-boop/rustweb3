unit TransferPanel;
interface
uses Winapi.Windows, System.SysUtils, System.Classes, System.JSON, System.SyncObjs, System.DateUtils,
  Vcl.Controls, Vcl.ExtCtrls, Vcl.StdCtrls, Vcl.Dialogs, Vcl.Graphics, Vcl.Forms, WalletTheme, SecureWalletApi, SolanaForm, TronForm;
type
  TTransferPanel = class(TPanel)
  private
    FApi: TSecureWalletApi;
    FTron: TTronForm;
    FNetwork: TComboBox;
    FNetworkIndex: Integer;
    FNetworkHost: TPanel;
    FPreviewMode: Boolean;
    FSolana: TSolanaForm;
    procedure NetworkChanged(Sender: TObject);
    function GetBusy: Boolean;
    function GetNavigationBusy: Boolean;
  private
    FProfile, FAddress, FId, FHash, FState: string;
    FSender, FRecipient, FAmount: TLabeledEdit;
    FPreview, FStatus: TMemo;
    FBalance, FSync: TLabel;
    FHome, FSendPage, FReceivePage, FHistoryPage: TPanel;
    FLocalPage, FDetailPage, FResultPage: TPanel;
    FResultTitle, FResultNote, FResultHash: TLabel;
    FTxJob, FSendJob, FRecoverJournal: Boolean;
    FNextTransaction: UInt64;
    procedure RenderTransactionResult;
    procedure CopyResultHash(Sender: TObject);
  private
    FChainList: TScrollBox;
    FChainInfo: TLabel;
    FHistoryNote: TLabel;
    FChainAddress: TLabel;
    FChainCopy: TButton;
    FChainMore, FChainRefresh, FExplorer: TButton;
    FHistoryAsset: TComboBox;
    FChainEntries: TJSONArray;
    FChainCursor, FSelectedHash: string;
    FChainQueued, FChainReset, FChainJob, FChainLoaded: Boolean;
    FChainScans: Integer;
    FChainReadCache: Boolean;
    FChainAuto: Boolean;
    FNextHistory: UInt64;
    FDetailText: TMemo;
    procedure QueueHistory(Sender: TObject);
    procedure HistoryAssetChanged(Sender: TObject);
    function HistoryAssetName: string;
    procedure LoadHistory;
    procedure OpenExplorer(Sender: TObject);
    procedure RenderHistory;
    procedure HistoryDetail(Sender: TObject);
    procedure CopyHash(Sender: TObject);
  private
    FAccountLabel, FHero, FReceiveAddress, FActionNote: TLabel;
    FUsdtRow, FBnbRow: TAssetRow;
    FOnManageWallet, FOnSelectAccount: TNotifyEvent;
    FBalanceFailures: Integer;
    FCurrentPage: Integer;
    procedure SelectAccount(Sender: TObject);
    procedure Navigate(Sender: TObject);
    procedure OpenBnbHistory(Sender: TObject);
    procedure ShowPage(Index: Integer);
    procedure ManageWallet(Sender: TObject);
    procedure StatusChanged(Sender: TObject);
  private
    FBalanceTimer: TTimer;
    FNextBalance: UInt64;
    FLastBalance: string;
    FPolling, FBalanceJob, FSuspendAuto: Boolean;
    FPrepare, FSend, FRefresh, FQuery, FReplay, FCancel, FCopy: TButton;
    FWorker: TThread;
    FAsset: TComboBox;
    FAll: TButton;
    FMaxBnb: Boolean;
    FBusy, FReady, FUnresolved: Boolean;
    FGeneration: Integer;
    FQuote: TJSONObject;
    FQuoteExpiry: UInt64;
    procedure FillAllAmount(const Balance: string);
    procedure SendAll(Sender: TObject);
    function MakeButton(const Text: string; TagValue, X, Y: Integer): TButton;
    procedure Action(Sender: TObject);
    procedure Changed(Sender: TObject);
    procedure CopyReceive(Sender: TObject);
    procedure UpdateButtons;
    procedure RunJob(const Work: TFunc<UTF8String>; const Done: TProc<UTF8String>; BalanceJob: Boolean = False);
    procedure BalanceTick(Sender: TObject);
    procedure ApplyBalance(Balance: TJSONObject);
    procedure BalanceFailed(const Failure: string);
    procedure RefreshAccount;
    procedure PrepareTransfer;
    procedure SendTransfer(Replay: Boolean);
    procedure QueryTransaction;
    procedure CancelPrepared;
    function AssetName: string;
  public
    constructor Create(AOwner: TComponent); override;
    destructor Destroy; override;
    procedure SetAccount(Api: TSecureWalletApi; const Profile, Address: string; const AccountName: string = '账户 01');
    function BeginAccountSelection: Boolean;
    procedure EndAccountSelection;
    procedure Lock;
    procedure ClearAccount;
    procedure RunReadOnlySelfTest(Api: TSecureWalletApi);
    procedure CheckLayout;
    procedure RunAmountSelfTest;
    procedure RunNetworkNavigationSelfTest;
    property Busy: Boolean read GetBusy;
    property OnManageWallet: TNotifyEvent read FOnManageWallet write FOnManageWallet;
    property OnSelectAccount: TNotifyEvent read FOnSelectAccount write FOnSelectAccount;
  end;
implementation
uses Vcl.Clipbrd;
const UsdtContract = '0x55d398326f99059fF775485246999027B3197955';
function HistoryState(Item: TJSONObject): string;
var State: string;
begin
  State := Item.GetValue<string>('state','finalized');
  if State='finalized' then Result := '已确认'
  else if State='reverted' then Result := '执行失败'
  else if State='included' then Result := '等待最终确认'
  else if State='signed' then Result := '已签名，尚未确认广播'
  else Result := '待确认';
end;
function TTransferPanel.AssetName: string;
begin
  if FAsset.ItemIndex=0 then Result := 'USDT' else Result := 'BNB';
end;
function JsonObject(const Text: UTF8String): TJSONObject;
var Value: TJSONValue;
begin
  Value := TJSONObject.ParseJSONValue(string(Text));
  if not (Value is TJSONObject) then begin Value.Free; raise Exception.Create('Invalid public response'); end;
  Result := TJSONObject(Value);
end;
function CanonicalAmount(const Text: string): string;
var Parts: TArray<string>; Fraction, Whole: string; C: Char;
begin
  Parts := Text.Split(['.']);
  if (Length(Parts) < 1) or (Length(Parts) > 2) then raise Exception.Create('金额格式错误');
  Whole := Parts[0]; Fraction := '';
  if Length(Parts) = 2 then Fraction := Parts[1];
  if (Length(Parts)=2) and (Fraction='') then raise Exception.Create('金额格式错误');
  if (Whole = '') or ((Length(Whole) > 1) and (Whole[1] = '0')) or (Length(Fraction) > 18) then raise Exception.Create('金额格式错误');
  for C in Whole + Fraction do if not CharInSet(C, ['0'..'9']) then raise Exception.Create('金额格式错误');
  Result := Whole + '.' + Fraction + StringOfChar('0', 18 - Length(Fraction));
end;
function FeeText(Price, Gas: UInt64): string;
var Fee: UInt64; Fraction: string;
begin
  if (Price = 0) or (Price > 1000000000000) then raise Exception.Create('手续费参数超出范围');
  if (Gas<21000) or (Gas>200000) then raise Exception.Create('Gas 超出支持范围');
  Fee := Price * Gas; Fraction := UIntToStr(Fee mod UInt64(1000000000000000000));
  Result := UIntToStr(Fee div UInt64(1000000000000000000)) + '.' + StringOfChar('0',18-Length(Fraction)) + Fraction;
end;
function HexBytes(const Bytes: UTF8String): UTF8String;
const Digits: AnsiString = '0123456789abcdef';
var I, B: Integer;
begin
  SetLength(Result, Length(Bytes)*2);
  for I := 1 to Length(Bytes) do begin B := Ord(Bytes[I]); Result[I*2-1] := Digits[(B shr 4)+1]; Result[I*2] := Digits[(B and 15)+1]; end;
end;
function TTransferPanel.MakeButton(const Text: string; TagValue, X, Y: Integer): TButton;
begin
  Result := TWalletButton.Create(Self); Result.Parent := Self; Result.SetBounds(X,Y,170,36);
  Result.Caption := Text; Result.Tag := TagValue; Result.OnClick := Action;
end;
constructor TTransferPanel.Create(AOwner: TComponent);
var L: TLabel; B: TButton; Header, Footer: TPanel; Mark: TWalletMark;
  function Page: TPanel;
  begin
    Result := TPanel.Create(Self); Result.Parent := Self; Result.Align := alClient;
    Result.BevelOuter := bvNone; Result.Color := clWhite; Result.ParentBackground := False;
    Result.DoubleBuffered := True;
  end;
  function Nav(Host: TWinControl; const Text: string; Index,X,Y,W: Integer): TButton;
  begin
    Result := MakeButton(Text,Index,X,Y); Result.Parent := Host;
    Result.Width := W; Result.Height := 48; Result.OnClick := Navigate;
  end;
begin
  inherited;
  if AOwner is TWinControl then Parent := TWinControl(AOwner);
  BevelOuter := bvNone; Align := alClient; Color := clWhite; ParentBackground := False; DoubleBuffered := True;
  Header := TPanel.Create(Self); Header.Parent := Self; Header.Align := alTop; Header.Height := 88;
  Header.BevelOuter := bvNone; Header.Color := clWhite; Header.ParentBackground := False;
  Mark := TWalletMark.Create(Self); Mark.Parent := Header; Mark.SetBounds(32,24,40,40);
  FAccountLabel := LabelText(Self,Header,'我的钱包',86,22,290,26,11,True);
  FAccountLabel.WordWrap := False; FAccountLabel.EllipsisPosition := epEndEllipsis;
  FAccountLabel.Cursor := crHandPoint; FAccountLabel.OnClick := SelectAccount;
  FNetwork := TComboBox.Create(Self); FNetwork.Parent := Header; FNetwork.SetBounds(86,50,320,28);
  FNetwork.Style := csDropDownList; FNetwork.Items.Add('BNB Smart Chain'); FNetwork.Items.Add('Solana'); FNetwork.Items.Add('Tron');
  FNetwork.ItemIndex := 0; FNetwork.OnChange := NetworkChanged;
  B := MakeButton('锁定',0,418,8); B.Parent := Header; B.SetBounds(418,8,90,30); B.OnClick := ManageWallet;
  Footer := TPanel.Create(Self); Footer.Parent := Self; Footer.Align := alBottom; Footer.Height := 116;
  Footer.BevelOuter := bvNone; Footer.Color := clWhite; Footer.ParentBackground := False;
  FActionNote := LabelText(Self,Footer,'',32,0,476,44,9,False,Muted);
  TWalletButton(Nav(Footer,'钱包',0,32,54,148)).Bare := True;
  TWalletButton(Nav(Footer,'交易记录',3,196,54,148)).Bare := True;
  B := Nav(Footer,'账户',0,360,54,148); TWalletButton(B).Bare := True; B.OnClick := SelectAccount;
  FHome := Page; FSendPage := Page; FReceivePage := Page; FHistoryPage := Page;
  FLocalPage := Page; FDetailPage := Page; FResultPage := Page;
  FNetworkHost := Page; FNetworkHost.Visible := False;
  Nav(FResultPage,'‹ 钱包',0,32,12,88);
  FResultTitle := LabelText(Self,FResultPage,'转账确认中',40,110,460,52,28,True);
  FResultNote := LabelText(Self,FResultPage,'',40,182,460,100,12,False,Muted);
  LabelText(Self,FResultPage,'交易哈希 · BNB Smart Chain',40,310,460,26,10,False,Muted);
  FResultHash := LabelText(Self,FResultPage,'',40,350,460,84,11,True);
  B := MakeButton('复制交易哈希',0,40,450); B.Parent := FResultPage;
  B.SetBounds(40,450,460,48); B.OnClick := CopyResultHash;
  Nav(FResultPage,'查看交易记录',3,40,520,460);
  FChainEntries := TJSONArray.Create;

  LabelText(Self,FHome,'我的资产',40,20,400,28,12,False,Muted);
  FHero := LabelText(Self,FHome,'—',36,56,468,66,36,True);
  LabelText(Self,FHome,'USDT',40,126,400,24,11,False,Muted);
  B := Nav(FHome,'发送',1,40,172,144); B.Height := 84; TWalletButton(B).ActionIcon := '↑';
  B := Nav(FHome,'接收',2,198,172,144); B.Height := 84; TWalletButton(B).ActionIcon := '↓';
  B := Nav(FHome,'记录',3,356,172,144); B.Height := 84; TWalletButton(B).ActionIcon := '↗';
  LabelText(Self,FHome,'币种',40,274,100,30,14,True);
  L := LabelText(Self,FHome,'余额',340,280,160,24,10,False,Muted); L.Alignment := taRightJustify;
  FUsdtRow := TAssetRow.Create(Self); FUsdtRow.Parent := FHome; FUsdtRow.Symbol := 'USDT'; FUsdtRow.SetBounds(40,322,460,88);
  FUsdtRow.Cursor := crHandPoint; FUsdtRow.Tag := 3; FUsdtRow.OnClick := Navigate;
  FBnbRow := TAssetRow.Create(Self); FBnbRow.Parent := FHome; FBnbRow.Symbol := 'BNB'; FBnbRow.SetBounds(40,420,460,88);
  FBnbRow.Cursor := crHandPoint; FBnbRow.OnClick := OpenBnbHistory;
  FBnbRow.Hint := '点击查看本机 BNB 发送记录'; FBnbRow.ShowHint := True;
  LabelText(Self,FHome,'BSC 主网',40,542,260,24,9,False,Muted);
  FSync := LabelText(Self,FHome,'',40,566,460,24,9,False,Muted); FSync.Visible := False;
  FBalance := TLabel.Create(Self); FBalance.Parent := FHome; FBalance.Visible := False; FBalance.Caption := '余额尚未查询';

  Nav(FSendPage,'‹ 返回',0,32,12,88);
  LabelText(Self,FSendPage,'发送',140,18,360,40,22,True);
  FAsset := TComboBox.Create(Self); FAsset.Parent := FSendPage; FAsset.SetBounds(40,82,460,32);
  FAsset.Style := csDropDownList; FAsset.Items.Add('USDT · BNB Smart Chain'); FAsset.Items.Add('BNB · BNB Smart Chain');
  FAsset.ItemIndex := 0; FAsset.OnChange := Changed;
  FRecipient := TWalletEdit.Create(Self); FRecipient.Parent := FSendPage; FlatEdit(FRecipient); FRecipient.SetBounds(40,152,460,30);
  FRecipient.EditLabel.Caption := '接收地址'; FRecipient.MaxLength := 42; FRecipient.OnChange := Changed; FRecipient.Font.Size := 10;
  FAmount := TWalletEdit.Create(Self); FAmount.Parent := FSendPage; FlatEdit(FAmount); FAmount.SetBounds(40,216,356,30);
  FAll := MakeButton('全部',0,410,212); FAll.Parent := FSendPage;
  FAll.SetBounds(410,212,90,38); FAll.OnClick := SendAll;
  FAmount.EditLabel.Caption := '数量 · USDT'; FAmount.MaxLength := 100; FAmount.OnChange := Changed;
  FPreview := TMemo.Create(Self); FPreview.Parent := FSendPage; FlatMemo(FPreview); FPreview.SetBounds(40,280,460,214); FPreview.ReadOnly := True;
  FPrepare := MakeButton('核对转账',2,40,520); FPrepare.Parent := FSendPage; FPrepare.SetBounds(40,520,222,48);
  FSend := MakeButton('确认并发送 USDT',3,278,520); FSend.Parent := FSendPage; FSend.SetBounds(278,520,222,48);
  TWalletButton(FSend).Primary := True;

  Nav(FReceivePage,'‹ 返回',0,32,12,88);
  LabelText(Self,FReceivePage,'接收',140,18,360,40,22,True);
  Mark := TWalletMark.Create(Self); Mark.Parent := FReceivePage; Mark.SetBounds(222,118,96,96);
  L := LabelText(Self,FReceivePage,'接收 USDT / BNB',40,248,460,40,20,True); L.Alignment := taCenter;
  L := LabelText(Self,FReceivePage,'BNB Smart Chain · BEP-20',40,302,460,28,11,False,Muted); L.Alignment := taCenter;
  FReceiveAddress := LabelText(Self,FReceivePage,'',48,364,444,72,12,True); FReceiveAddress.Alignment := taCenter;
  FSender := TWalletEdit.Create(Self); FSender.Parent := FReceivePage; FSender.Visible := False; FSender.EditLabel.Visible := False; FSender.ReadOnly := True;
  FCopy := MakeButton('复制收款地址',7,40,468); FCopy.Parent := FReceivePage; FCopy.SetBounds(40,468,460,52);
  TWalletButton(FCopy).Primary := True; FCopy.OnClick := CopyReceive;
  L := LabelText(Self,FReceivePage,'请让转出方选择 BSC 网络',40,540,460,28,10,False,Muted); L.Alignment := taCenter;

  Nav(FHistoryPage,'‹ 返回',0,32,12,88);
  FHistoryAsset := TComboBox.Create(Self); FHistoryAsset.Parent := FHistoryPage;
  FHistoryAsset.SetBounds(140,20,180,32); FHistoryAsset.Style := csDropDownList;
  FHistoryAsset.Items.Add('USDT'); FHistoryAsset.Items.Add('BNB'); FHistoryAsset.ItemIndex := 0;
  FHistoryAsset.OnChange := HistoryAssetChanged;
  B := Nav(FHistoryPage,'本机转账',4,382,16,118); B.Height := 40;
  FHistoryNote := LabelText(Self,FHistoryPage,'BNB Smart Chain · 链上收发记录',40,76,460,26,10,False,Muted);
  B := Nav(FHistoryPage,'发送',1,40,106,222); B.Height := 76; TWalletButton(B).ActionIcon := '↑';
  B := Nav(FHistoryPage,'接收',2,278,106,222); B.Height := 76; TWalletButton(B).ActionIcon := '↓';
  LabelText(Self,FHistoryPage,'我的收款地址',40,190,340,22,10,False,Muted);
  FChainAddress := LabelText(Self,FHistoryPage,'',40,216,340,48,10,True);
  FChainCopy := MakeButton('复制地址',0,392,218); FChainCopy.Parent := FHistoryPage;
  FChainCopy.SetBounds(392,218,108,40); FChainCopy.OnClick := CopyReceive;
  FChainInfo := LabelText(Self,FHistoryPage,'',40,278,460,28,10,False,Muted);
  FChainList := TScrollBox.Create(Self); FChainList.Parent := FHistoryPage;
  FChainList.SetBounds(32,310,476,216); FChainList.BorderStyle := bsNone;
  FChainList.Color := clWhite; FChainList.HorzScrollBar.Visible := False;
  FChainRefresh := MakeButton('刷新',0,40,542); FChainRefresh.Parent := FHistoryPage;
  FChainRefresh.SetBounds(40,542,144,44); FChainRefresh.OnClick := QueueHistory;
  FChainMore := MakeButton('更早记录',1,200,542); FChainMore.Parent := FHistoryPage;
  FChainMore.SetBounds(200,542,300,44); FChainMore.OnClick := QueueHistory;
  FExplorer := MakeButton('浏览器查看完整记录',0,276,542);
  FExplorer.Parent := FHistoryPage; FExplorer.SetBounds(276,542,224,44);
  FExplorer.OnClick := OpenExplorer; FExplorer.Visible := False;
  Nav(FDetailPage,'‹ 返回',3,32,12,88);
  LabelText(Self,FDetailPage,'交易详情',140,18,360,40,22,True);
  FDetailText := TMemo.Create(Self); FDetailText.Parent := FDetailPage; FlatMemo(FDetailText);
  FDetailText.SetBounds(40,90,460,410); FDetailText.ReadOnly := True;
  B := MakeButton('复制交易哈希',0,40,530); B.Parent := FDetailPage;
  B.SetBounds(40,530,460,48); B.OnClick := CopyHash;
  Nav(FLocalPage,'‹ 返回',3,32,12,88);
  LabelText(Self,FLocalPage,'本机转账',140,18,360,40,22,True);
  LabelText(Self,FLocalPage,'待处理与本机发起的交易',40,86,460,26,10,False,Muted);
  FStatus := TMemo.Create(Self); FStatus.Parent := FLocalPage; FlatMemo(FStatus); FStatus.SetBounds(40,130,460,270);
  FStatus.ReadOnly := True; FStatus.Text := '暂无本机交易记录'; FStatus.OnChange := StatusChanged;
  FRefresh := MakeButton('刷新记录',1,40,426); FRefresh.Parent := FLocalPage; FRefresh.SetBounds(40,426,222,44);
  FQuery := MakeButton('查询交易结果',4,278,426); FQuery.Parent := FLocalPage; FQuery.SetBounds(278,426,222,44);
  FReplay := MakeButton('重播原交易',5,40,486); FReplay.Parent := FLocalPage; FReplay.SetBounds(40,486,222,44);
  FCancel := MakeButton('取消未签名交易',6,278,486); FCancel.Parent := FLocalPage; FCancel.SetBounds(278,486,222,44);
  FBalanceTimer := TTimer.Create(Self); FBalanceTimer.Interval := 250; FBalanceTimer.OnTimer := BalanceTick;
  ShowPage(0); UpdateButtons;
end;
procedure TTransferPanel.ShowPage(Index: Integer);
begin
  if FNetworkIndex<>0 then
  begin
    if Assigned(FSolana) and (FNetworkIndex=1) then FSolana.SelectPage(Index);
    if Assigned(FTron) and (FNetworkIndex=2) then FTron.SelectPage(Index);
    Exit;
  end;
  FCurrentPage := Index;
  FHome.Visible := Index=0; FSendPage.Visible := Index=1;
  FReceivePage.Visible := Index=2; FHistoryPage.Visible := Index=3;
  FLocalPage.Visible := Index=4; FDetailPage.Visible := Index=5; FResultPage.Visible := Index=6;
  FActionNote.Visible := Index=1;
end;
procedure TTransferPanel.RenderTransactionResult;
begin
  FResultHash.Caption := FHash;
  FResultTitle.Font.Color := clBlack;
  if FState='finalized' then
  begin
    FResultTitle.Caption := '转账成功';
    FResultTitle.Font.Color := $00806000;
    FResultNote.Caption := '链上已确认。余额和交易记录将自动更新。';
  end
  else if FState='reverted' then
  begin
    FResultTitle.Caption := '转账失败';
    FResultTitle.Font.Color := clMaroon;
    FResultNote.Caption := '链上执行失败，转账未完成，已消耗的网络手续费仍会扣除。';
  end
  else
  begin
    FResultTitle.Caption := '转账确认中';
    if FState='included' then FResultNote.Caption := '交易已入块，正在等待最终确认。'
    else FResultNote.Caption := '正在自动查询链上结果。可以返回钱包，确认会在后台继续。';
  end;
end;
procedure TTransferPanel.CopyResultHash(Sender: TObject);
begin
  if FHash<>'' then begin Clipboard.AsText := FHash; TButton(Sender).Caption := '已复制'; end;
end;
procedure TTransferPanel.Navigate(Sender: TObject);
begin
  if FNetworkIndex<>0 then begin ShowPage(TControl(Sender).Tag); Exit; end;
  if Sender=FUsdtRow then begin FHistoryAsset.ItemIndex := 0; HistoryAssetChanged(nil); end;
  if (FCurrentPage=3) and (TControl(Sender).Tag=1) and (FAsset.ItemIndex<>FHistoryAsset.ItemIndex) then
  begin FAsset.ItemIndex := FHistoryAsset.ItemIndex; FAmount.Clear; Changed(nil); end;
  ShowPage(TControl(Sender).Tag);
  if (FCurrentPage=3) and not FChainJob then
  begin
    FChainAuto := False; FChainReset := True; FChainQueued := True; FChainScans := 0;
    if not FChainLoaded then FChainInfo.Caption := '正在加载记录…';
  end;
end;
function TTransferPanel.HistoryAssetName: string;
begin
  if FHistoryAsset.ItemIndex=1 then Result := 'BNB' else Result := 'USDT';
end;
procedure TTransferPanel.HistoryAssetChanged(Sender: TObject);
begin
  if FHistoryAsset.ItemIndex=1 then
    FHistoryNote.Caption := 'BNB · 本机发送记录'
  else FHistoryNote.Caption := 'BNB Smart Chain · USDT 收发记录';
  FExplorer.Visible := FHistoryAsset.ItemIndex=1;
  if FHistoryAsset.ItemIndex=1 then
  begin
    FChainRefresh.Width := MulDiv(96,CurrentPPI,96);
    FChainMore.Left := MulDiv(148,CurrentPPI,96); FChainMore.Width := MulDiv(116,CurrentPPI,96);
  end
  else
  begin
    FChainRefresh.Width := MulDiv(144,CurrentPPI,96);
    FChainMore.Left := MulDiv(200,CurrentPPI,96); FChainMore.Width := MulDiv(300,CurrentPPI,96);
  end;
  FChainEntries.Free; FChainEntries := TJSONArray.Create; FChainCursor := '';
  FChainLoaded := False; FChainReadCache := True; FChainReset := True;
  FChainQueued := True; FChainAuto := False; FChainScans := 0;
  FSelectedHash := ''; FDetailText.Clear; RenderHistory;
  FChainInfo.Caption := '正在加载 '+HistoryAssetName+' 记录…';
end;
procedure TTransferPanel.OpenBnbHistory(Sender: TObject);
begin
  if FBusy and not FBalanceJob then Exit;
  if FAddress='' then Exit;
  FHistoryAsset.ItemIndex := 1; HistoryAssetChanged(nil);
  ShowPage(3);
end;
procedure TTransferPanel.ManageWallet(Sender: TObject);
begin
  if Assigned(FOnManageWallet) then FOnManageWallet(Self);
end;
procedure TTransferPanel.SelectAccount(Sender: TObject);
begin
  if Assigned(FOnSelectAccount) then FOnSelectAccount(Self);
end;
function TTransferPanel.BeginAccountSelection: Boolean;
var Deadline: UInt64;
begin
  Result := False;
  if (GetBusy and not FBusy) or (FBusy and not FBalanceJob) then
  begin FActionNote.Caption := '请等待当前操作完成后切换账户。'; Exit; end;
  if Assigned(FSolana) then FSolana.Pause(True);
  if Assigned(FTron) then FTron.Pause(True);
  FSuspendAuto := True; Deadline := GetTickCount64+55000;
  while FBusy and (GetTickCount64<Deadline) do begin Application.ProcessMessages; Sleep(5); end;
  Result := not FBusy;
  if not Result then FSuspendAuto := False;
end;
procedure TTransferPanel.EndAccountSelection;
begin
  FSuspendAuto := FNetworkIndex<>0;
  if Assigned(FSolana) then FSolana.Pause(FNetworkIndex<>1);
  if Assigned(FTron) then FTron.Pause(FNetworkIndex<>2);
end;
procedure TTransferPanel.StatusChanged(Sender: TObject);
begin
  if Assigned(FActionNote) then
    if Trim(FStatus.Text)='暂无本机交易记录' then FActionNote.Caption := ''
    else FActionNote.Caption := FStatus.Text;
end;
procedure TTransferPanel.QueueHistory(Sender: TObject);
begin
  if FChainJob or (FAddress='') then Exit;
  FChainAuto := False; FChainReadCache := False; FChainReset := TButton(Sender).Tag=0; FChainScans := 0; FChainQueued := True;
  FChainInfo.Caption := '正在加载记录…';
end;
procedure TTransferPanel.OpenExplorer(Sender: TObject);
var Account: UTF8String;
begin
  if FBusy or (FAddress='') then Exit;
  Account := UTF8String(FAddress);
  RunJob(function: UTF8String
    begin
      try Result := FApi.PublicRequest(12,[Account]);
      except Result := ''; end;
    end,
    procedure(Value: UTF8String)
    begin
      if Value='' then FChainInfo.Caption := '浏览器未能打开，请复制地址后到 bscscan.com 查询'
      else FChainInfo.Caption := '已请求打开浏览器，查看完整 BNB 收发记录';
    end);
end;
procedure TTransferPanel.LoadHistory;
var Account, Cursor: UTF8String; Reset, CacheOnly: Boolean; Asset: string; Epoch: UInt64;
begin
  if FBusy or (FAddress='') then Exit;
  Account := UTF8String(FAddress); Reset := FChainReset; CacheOnly := FChainReadCache;
  Asset := HistoryAssetName;
  Epoch := FApi.Epoch;
  if Reset then Cursor := '' else Cursor := UTF8String(FChainCursor);
  FChainQueued := False; FChainJob := True;
  RunJob(
    function: UTF8String begin
      if Asset='BNB' then
        Result := FApi.RequestAt(Epoch,19,[Account,Cursor])
      else if CacheOnly then Result := FApi.PublicRequest(8,[Account,''])
      else Result := FApi.PublicRequest(7,[Account,Cursor]);
    end,
    procedure(Value: UTF8String)
    var Data: TJSONObject; Entries: TJSONArray; Item, Old: TJSONObject; I,J: Integer; Duplicate: Boolean; Before: string;
    begin
      if Asset<>HistoryAssetName then Exit;
      Data := JsonObject(Value);
      try
        if (Data.GetValue<Integer>('chain_id')<>56) or (Data.GetValue<string>('asset')<>Asset) or
          not SameText(Data.GetValue<string>('account'),FAddress) then
          raise Exception.Create('记录与当前钱包不一致');
        if Asset='BNB' then
        begin
          if not Data.GetValue<Boolean>('local',False) then raise Exception.Create('无效的本机记录');
        end
        else if not Data.GetValue<Boolean>('finalized') then raise Exception.Create('链上记录尚未确认');
        Entries := Data.GetValue<TJSONArray>('entries');
        if Entries.Count>3 then raise Exception.Create('链上记录分页无效');
        Before := FChainEntries.ToJSON;
        if Reset then begin FChainEntries.Free; FChainEntries := TJSONArray.Create; end;
        for I := 0 to Entries.Count-1 do
        begin
          Item := Entries.Items[I] as TJSONObject; Duplicate := False;
          for J := 0 to FChainEntries.Count-1 do
          begin
            Old := FChainEntries.Items[J] as TJSONObject;
            if (Old.GetValue<string>('h')=Item.GetValue<string>('h')) and
              (Old.GetValue<Int64>('i')=Item.GetValue<Int64>('i')) then Duplicate := True;
          end;
          if not Duplicate then FChainEntries.AddElement(TJSONObject.ParseJSONValue(Item.ToJSON));
        end;
        FChainCursor := Data.GetValue<string>('next'); FChainLoaded := True; Inc(FChainScans);
        FChainReset := False;
        if Before<>FChainEntries.ToJSON then RenderHistory;
        if Asset='BNB' then
        begin
          FChainReadCache := False;
          if FChainEntries.Count=0 then FChainInfo.Caption := '暂无本机 BNB 发送记录；完整收发记录请在浏览器查看'
          else FChainInfo.Caption := '仅本机发送；外部转入和其他设备记录请在浏览器查看';
          Exit;
        end;
        if FChainEntries.Count=0 then FChainInfo.Caption := '本段暂无记录，可继续查看更早记录'
        else FChainInfo.Caption := '已确认的 '+Asset+' 收发记录';
        if CacheOnly then
        begin
          FChainReadCache := False; FChainReset := True; FChainQueued := True; FChainScans := 0;
          if FChainEntries.Count>0 then FChainInfo.Caption := '已保存的 '+Asset+' 收发记录';
        end
        else if Data.GetValue<Boolean>('more_sync',False) and (FChainScans<4) then
        begin FChainReset := True; FChainQueued := True; end
        else if not FChainAuto and (FChainCursor<>'') and (Entries.Count=0) and (FChainScans<4) and (FCurrentPage=3) then
          FChainQueued := True;
      finally Data.Free; end;
    end);
end;
procedure TTransferPanel.RenderHistory;
var I,Y: Integer; Day, LastDay, Direction, Counterparty, Amount, SignText, Icon: string;
    Item: TJSONObject; Row: TPanel; L: TLabel; B: TWalletButton; Tint: TColor;
  function P(N: Integer): Integer;
  begin Result := MulDiv(N,CurrentPPI,96); end;
  function ShortAddress(const Value: string): string;
  begin Result := Copy(Value,1,8)+'…'+Copy(Value,Length(Value)-3,4); end;
begin
  FChainList.DisableAlign;
  try
    while FChainList.ControlCount>0 do FChainList.Controls[0].Free;
    Y := 0; LastDay := '';
    for I := 0 to FChainEntries.Count-1 do
    begin
      Item := FChainEntries.Items[I] as TJSONObject;
      if Item.GetValue<Int64>('t')=0 then Day := '本机发送 · 最近记录在前'
      else Day := FormatDateTime('yyyy/mm/dd',UnixToDateTime(Item.GetValue<Int64>('t'),False));
      if Day<>LastDay then
      begin
        L := LabelText(FChainList,FChainList,Day,0,0,1,1,10,False,Muted);
        L.SetBounds(P(8),P(Y+8),P(440),P(24)); Inc(Y,42); LastDay := Day;
      end;
      Direction := '发送'; SignText := '-'; Icon := '↑'; Tint := Ink;
      Counterparty := '至 '+ShortAddress(Item.GetValue<string>('to'));
      if SameText(Item.GetValue<string>('to'),FAddress) then
      begin
        Direction := '接收'; SignText := '+'; Icon := '↓'; Tint := $003A7D17;
        Counterparty := '来自 '+ShortAddress(Item.GetValue<string>('from'));
        if SameText(Item.GetValue<string>('from'),FAddress) then
        begin Direction := '自转'; SignText := ''; Icon := '↔'; Tint := Ink; end;
      end;
      Amount := Item.GetValue<string>('a');
      if Pos('.',Amount)>0 then
      begin
        while (Length(Amount)>0) and (Amount[Length(Amount)]='0') do Delete(Amount,Length(Amount),1);
        if (Length(Amount)>0) and (Amount[Length(Amount)]='.') then Delete(Amount,Length(Amount),1);
      end;
      Row := TPanel.Create(FChainList); Row.Parent := FChainList;
      Row.SetBounds(P(8),P(Y),P(442),P(86)); Row.BevelOuter := bvNone;
      Row.Color := clWhite; Row.ParentBackground := False;
      B := TWalletButton.Create(Row); B.Parent := Row; B.SetBounds(0,P(10),P(42),P(42));
      B.Caption := Icon; B.Tag := I; B.OnClick := HistoryDetail;
      L := LabelText(Row,Row,Direction,0,0,1,1,12,True); L.SetBounds(P(56),P(10),P(110),P(26));
      L.Tag := I; L.OnClick := HistoryDetail; L.Cursor := crHandPoint;
      L := LabelText(Row,Row,Counterparty,0,0,1,1,10,False,Muted); L.SetBounds(P(56),P(42),P(220),P(24));
      L.Tag := I; L.OnClick := HistoryDetail; L.Cursor := crHandPoint;
      L := LabelText(Row,Row,SignText+Amount+' '+HistoryAssetName,0,0,1,1,11,True,Tint); L.SetBounds(P(180),P(10),P(262),P(26));
      L.Alignment := taRightJustify; L.WordWrap := False; L.EllipsisPosition := epEndEllipsis;
      L.Hint := L.Caption; L.ShowHint := True; L.Tag := I; L.OnClick := HistoryDetail; L.Cursor := crHandPoint;
      L := LabelText(Row,Row,HistoryState(Item)+'  ›',0,0,1,1,9,False,Muted); L.SetBounds(P(270),P(42),P(172),P(24));
      L.Alignment := taRightJustify; L.Tag := I; L.OnClick := HistoryDetail; L.Cursor := crHandPoint;
      Inc(Y,86);
    end;
    FChainList.VertScrollBar.Range := P(Y+8);
  finally FChainList.EnableAlign; end;
end;
procedure TTransferPanel.HistoryDetail(Sender: TObject);
var Item: TJSONObject; Index: Integer; Direction: string;
begin
  Index := TControl(Sender).Tag;
  if (Index<0) or (Index>=FChainEntries.Count) then Exit;
  Item := FChainEntries.Items[Index] as TJSONObject;
  FSelectedHash := Item.GetValue<string>('h');
  if SameText(Item.GetValue<string>('to'),FAddress) then Direction := '接收' else Direction := '发送';
  if SameText(Item.GetValue<string>('to'),FAddress) and SameText(Item.GetValue<string>('from'),FAddress) then Direction := '自转';
  FDetailText.Text := Direction+' '+HistoryAssetName+sLineBreak+Item.GetValue<string>('a')+' '+HistoryAssetName+sLineBreak+sLineBreak+
    '状态：'+HistoryState(Item)+sLineBreak+'网络：BNB Smart Chain'+sLineBreak+sLineBreak+
    '发送地址：'+sLineBreak+Item.GetValue<string>('from')+sLineBreak+sLineBreak+
    '接收地址：'+sLineBreak+Item.GetValue<string>('to')+sLineBreak+sLineBreak+
    '交易哈希：'+sLineBreak+FSelectedHash;
  if Item.GetValue<Int64>('t')>0 then
    FDetailText.Lines.Add(sLineBreak+'时间：'+FormatDateTime('yyyy/mm/dd hh:nn:ss',UnixToDateTime(Item.GetValue<Int64>('t'),False))+
      sLineBreak+'区块：'+IntToStr(Item.GetValue<Int64>('b')))
  else FDetailText.Lines.Add(sLineBreak+'来源：本机签名账本。链上时间和完整收发记录请在浏览器查看。');
  ShowPage(5);
end;
procedure TTransferPanel.CopyHash(Sender: TObject);
begin
  if FSelectedHash='' then Exit;
  try Clipboard.AsText := FSelectedHash; TButton(Sender).Caption := '已复制交易哈希';
  except TButton(Sender).Caption := '复制失败，请重试'; end;
end;
destructor TTransferPanel.Destroy;
begin
  if Assigned(FBalanceTimer) then FBalanceTimer.Enabled := False;
  FQuote.Free; FWorker.Free; FChainEntries.Free;
  inherited;
end;
function TTransferPanel.GetBusy: Boolean;
begin Result := FBusy or (Assigned(FSolana) and FSolana.Busy) or (Assigned(FTron) and FTron.Busy); end;
function TTransferPanel.GetNavigationBusy: Boolean;
begin
  Result := (FBusy and not (FBalanceJob or FChainJob)) or
    (Assigned(FSolana) and FSolana.NavigationBusy) or
    (Assigned(FTron) and FTron.NavigationBusy);
end;
procedure TTransferPanel.NetworkChanged(Sender: TObject);
var Target: Integer;
begin
  Target := FNetwork.ItemIndex;
  if (Target=FNetworkIndex) then Exit;
  if GetNavigationBusy or (FAddress='') then begin FNetwork.ItemIndex := FNetworkIndex; Exit; end;
  Changed(nil); FRecipient.Clear; FAmount.Clear;
  if Assigned(FSolana) then begin FSolana.Pause(True); FSolana.Hide; end;
  if Assigned(FTron) then begin FTron.Pause(True); FTron.Hide; end;
  FChainQueued := False;
  FNetworkIndex := 0; ShowPage(0); FNetworkHost.Visible := False;
  FSuspendAuto := Target<>0;
  try
    if Target=1 then
    begin
      if not Assigned(FSolana) then
      begin
        FSolana := TSolanaForm.Create(Self); FSolana.Parent := FNetworkHost; FSolana.Align := alClient;
        FSolana.ScaleForPPI(CurrentPPI);
        if FPreviewMode or SameText(ParamStr(1),'--public-preview') then FSolana.PublicPreview
        else FSolana.OpenAccount(FApi,FProfile,FAccountLabel.Caption);
      end;
      FSolana.Pause(False); FSolana.Show;
    end
    else if Target=2 then
    begin
      if not Assigned(FTron) then
      begin
        FTron := TTronForm.Create(Self); FTron.Parent := FNetworkHost; FTron.Align := alClient;
        FTron.ScaleForPPI(CurrentPPI);
        if FPreviewMode or SameText(ParamStr(1),'--public-preview') then FTron.PublicPreview
        else FTron.OpenAccount(FApi,FProfile,FAccountLabel.Caption);
      end;
      FTron.Pause(False); FTron.Show;
    end;
    FNetworkIndex := Target;
    if Target<>0 then
    begin
      FHome.Hide; FSendPage.Hide; FReceivePage.Hide; FHistoryPage.Hide;
      FLocalPage.Hide; FDetailPage.Hide; FResultPage.Hide; FActionNote.Hide;
      FNetworkHost.Show; FNetworkHost.BringToFront;
    end;
    ShowPage(0);
  except
    on E: Exception do
    begin
      if (Target=1) and Assigned(FSolana) and not FSolana.Busy then FreeAndNil(FSolana);
      if (Target=2) and Assigned(FTron) and not FTron.Busy then FreeAndNil(FTron);
      FNetworkIndex := 0; FNetwork.ItemIndex := 0; FSuspendAuto := False; FNetworkHost.Hide; ShowPage(0);
      FActionNote.Caption := E.Message; FActionNote.Show;
    end;
  end;
  UpdateButtons;
end;
procedure TTransferPanel.CopyReceive(Sender: TObject);
begin
  if not FCopy.Enabled then Exit;
  try
    Clipboard.AsText := FAddress;
    FCopy.Caption := '已复制 BSC 地址'; FChainCopy.Caption := '已复制';
  except
    on E: Exception do begin FCopy.Caption := '复制失败，请重试'; FChainCopy.Caption := '复制失败'; end;
  end;
end;
procedure TTransferPanel.UpdateButtons;
begin
  FNetwork.Enabled := not GetNavigationBusy and (FAddress<>'');
  FHistoryAsset.Enabled := not FBusy;
  FExplorer.Enabled := not FBusy and (FAddress<>'');
  FAll.Visible := True;
  FAll.Enabled := not FBusy and FReady and not FUnresolved and (FAddress<>'') and
    not SameText(ParamStr(1),'--public-preview');
  FChainRefresh.Enabled := not FBusy and not FChainQueued;
  FChainMore.Enabled := not FBusy and not FChainQueued and FChainLoaded and (FChainCursor<>'') and
    ((FHistoryAsset.ItemIndex=1) or (FChainEntries.Count<300));
  FCopy.Enabled := (Length(FAddress)=42) and not SameText(ParamStr(1),'--public-preview');
  FChainCopy.Enabled := FCopy.Enabled;
  FRefresh.Enabled := not FBusy and (FAddress <> '');
  FPrepare.Enabled := not FBusy and FReady and not FUnresolved;
  FSend.Enabled := not FBusy and FReady and not FUnresolved and Assigned(FQuote);
  FQuery.Enabled := not FBusy and (FHash <> '');
  FReplay.Enabled := not FBusy and FReady and ((FState='signed') or (FState='broadcasting') or (FState='unknown'));
  FCancel.Enabled := not FBusy and (FState='prepared');
  FRecipient.Enabled := not FBusy or FBalanceJob; FAmount.Enabled := FRecipient.Enabled;
  FAsset.Enabled := FRecipient.Enabled;
end;
procedure TTransferPanel.FillAllAmount(const Balance: string);
var Amount: string;
begin
  Amount := CanonicalAmount(Balance);
  if Amount='0.000000000000000000' then
  begin
    FAmount.Clear; Changed(nil);
    FStatus.Text := 'USDT 余额为 0，暂无可发送数量。'; Exit;
  end;
  // Keep every decimal digit; never use floating point or the rounded display balance.
  FAmount.Text := Amount; Changed(nil);
  FStatus.Text := '已填入全部 USDT。手续费另用 BNB 支付，请点击“核对转账”。';
end;

procedure TTransferPanel.SendAll(Sender: TObject);
var Account: UTF8String;
begin
  if FBusy or not FReady or FUnresolved or (FAddress='') or
    SameText(ParamStr(1),'--public-preview') then Exit;
  if FAsset.ItemIndex=1 then
  begin
    if Trim(FRecipient.Text)='' then
    begin FStatus.Text := '请先填写收款地址，再点击“全部”。'; FRecipient.SetFocus; Exit; end;
    FAmount.Clear; Changed(nil); FMaxBnb := True;
    FStatus.Text := '正在计算扣除网络手续费后的全部 BNB…';
    try PrepareTransfer; except on E: Exception do begin FMaxBnb := False; FStatus.Text := E.Message; end; end;
    Exit;
  end;
  Account := UTF8String(FAddress);
  Changed(nil); FStatus.Text := '正在查询可发送的 USDT 余额…';
  RunJob(
    function: UTF8String begin Result := FApi.PublicRequest(5,[Account]); end,
    procedure(Value: UTF8String)
    var Data: TJSONObject;
    begin
      Data := JsonObject(Value);
      try
        ApplyBalance(Data);
        FillAllAmount(Data.GetValue<string>('usdt_balance'));
      finally Data.Free; end;
    end);
end;

procedure TTransferPanel.RunAmountSelfTest;
var Item: TJSONObject;
begin
  FillAllAmount('123456789.123456789012345678');
  if FAmount.Text<>'123456789.123456789012345678' then raise Exception.Create('All USDT precision failed');
  FQuote := TJSONObject.Create; FQuoteExpiry := GetTickCount64+30000;
  FillAllAmount('0.000000000000000001');
  if (FAmount.Text<>'0.000000000000000001') or Assigned(FQuote) or (FQuoteExpiry<>0) then
    raise Exception.Create('All USDT quote invalidation failed');
  FillAllAmount('0');
  if FAmount.Text<>'' then raise Exception.Create('Zero USDT handling failed');
  FAsset.ItemIndex := 1; Changed(nil);
  if not FAll.Visible then raise Exception.Create('BNB maximum unavailable');
  FAsset.ItemIndex := 0; Changed(nil); ClearAccount;
  if FAll.Enabled then raise Exception.Create('All USDT available while locked');
  FHistoryAsset.ItemIndex := 1; HistoryAssetChanged(nil);
  Item := TJSONObject.Create;
  Item.AddPair('h','0x'+StringOfChar('1',64)); Item.AddPair('i',TJSONNumber.Create(0));
  Item.AddPair('b',TJSONNumber.Create(0)); Item.AddPair('t',TJSONNumber.Create(0));
  Item.AddPair('state','reverted');
  Item.AddPair('from','0x1111111111111111111111111111111111111111');
  Item.AddPair('to','0x2222222222222222222222222222222222222222'); Item.AddPair('a','0.100000000000000000');
  FChainEntries.AddElement(Item); RenderHistory; HistoryDetail(FChainRefresh);
  if Pos('BNB',FDetailText.Text)=0 then raise Exception.Create('BNB history detail asset failed');
  if (Pos('执行失败',FDetailText.Text)=0) or (Pos('1970',FDetailText.Text)>0) or not FExplorer.Visible then
    raise Exception.Create('Local BNB status, unknown time or explorer failed');
  FHistoryAsset.ItemIndex := 0; HistoryAssetChanged(nil);
  if FExplorer.Visible then raise Exception.Create('BNB explorer visible for USDT');
  if (FChainEntries.Count<>0) or (FSelectedHash<>'') then raise Exception.Create('History asset switch retained foreign entries');
  ClearAccount; ShowPage(0);
end;
procedure TTransferPanel.Lock;
begin
  if Assigned(FSolana) then FSolana.Lock;
  if Assigned(FTron) then FTron.Lock;
  FNetworkIndex := 0; FNetwork.ItemIndex := 0; FNetworkHost.Hide; FSuspendAuto := False;
  FChainQueued := False;
  FPolling := False; FRecoverJournal := False; FResultHash.Caption := ''; FResultNote.Caption := '';
  TInterlocked.Increment(FGeneration);
  FPreview.Clear; FreeAndNil(FQuote); FQuoteExpiry := 0;
  FRecipient.Clear; FAmount.Clear;
  FReady := False; UpdateButtons;
  FSync.Visible := False;
end;
procedure TTransferPanel.Changed(Sender: TObject);
begin
  FMaxBnb := False;
  if Sender=FAsset then FAmount.Clear;
  FAmount.EditLabel.Caption := '数量 · '+AssetName;
  FSend.Caption := '确认并发送 '+AssetName;
  FreeAndNil(FQuote); FQuoteExpiry := 0; FPreview.Clear; UpdateButtons;
end;
procedure TTransferPanel.ClearAccount;
begin
  Lock; FProfile := ''; FAddress := ''; FSender.Clear; FId := ''; FHash := ''; FState := '';
  FBalance.Caption := '余额尚未查询'; FLastBalance := '';
  FHero.Caption := '—'; FUsdtRow.Amount := '—'; FBnbRow.Amount := '—';
  FReceiveAddress.Caption := ''; FChainAddress.Caption := ''; FChainCopy.Caption := '复制地址';
  FAccountLabel.Caption := '我的钱包'; FAccountLabel.Hint := ''; FBalanceFailures := 0;
  FUsdtRow.Hint := ''; FBnbRow.Hint := '';
  FChainEntries.Free; FChainEntries := TJSONArray.Create; FChainCursor := ''; FChainLoaded := False;
  FChainInfo.Caption := ''; FSelectedHash := ''; FDetailText.Clear; RenderHistory;
  FStatus.Text := '先在“钱包与备份”页验证当前选定钱包的地址。'; UpdateButtons;
end;
procedure TTransferPanel.SetAccount(Api: TSecureWalletApi; const Profile, Address: string; const AccountName: string);
begin
  if GetBusy then raise Exception.Create('请等待当前请求完成');
  FreeAndNil(FSolana); FreeAndNil(FTron);
  Lock; FApi := Api; FProfile := Profile; FAddress := Address; FSender.Text := Address; FCopy.Caption := '复制我的收款地址';
  if SameText(ParamStr(1),'--public-preview') then FSender.Text := 'PUBLIC TEST PREVIEW - DO NOT FUND';
  FId := ''; FHash := ''; FState := ''; FUnresolved := True;
  FBalance.Caption := '余额尚未查询'; FLastBalance := ''; FBalanceFailures := 0;
  FChainEntries.Free; FChainEntries := TJSONArray.Create; FChainCursor := ''; FChainLoaded := False;
  FChainReadCache := True; FChainQueued := False; FChainScans := 0; FChainInfo.Caption := ''; RenderHistory;
  FAccountLabel.Caption := Profile+' · '+AccountName+' ▾'; FAccountLabel.Hint := FAccountLabel.Caption; FAccountLabel.ShowHint := True;
  FReceiveAddress.Caption := Address;
  FChainAddress.Caption := Copy(Address,1,24)+sLineBreak+Copy(Address,25,18);
  FChainCopy.Caption := '复制地址';
  if SameText(ParamStr(1),'--public-preview') then
  begin FAccountLabel.Caption := '公开预览 · 不可充值'; FReceiveAddress.Caption := '公开测试预览，不可充值';
    FChainAddress.Caption := '公开测试预览，不可充值'; end;
  ShowPage(0);
  FPolling := not SameText(ParamStr(1),'--public-preview');
  RefreshAccount;
end;
procedure TTransferPanel.RunJob(const Work: TFunc<UTF8String>; const Done: TProc<UTF8String>; BalanceJob: Boolean);
var Generation: Integer;
begin
  if FBusy then Exit;
  FBusy := True; FBalanceJob := BalanceJob; Generation := FGeneration; UpdateButtons;
  FWorker := TThread.CreateAnonymousThread(
    procedure
    var Value: UTF8String; Failure: string;
    begin
      try Value := Work(); except on E: Exception do Failure := E.Message; end;
      TThread.Queue(nil,
        procedure
        var Completed: TThread;
        begin
          Completed := FWorker; FWorker := nil; Completed.Free;
          FBusy := False;
          try
            if Generation = FGeneration then
            begin
              if (Failure <> '') and FChainJob then
              begin FChainQueued := False; FChainInfo.Caption := '记录暂时无法加载，请点击刷新重试'; end
              else if (Failure <> '') and (FTxJob or FSendJob) then
              begin
                if FSendJob then begin FRecoverJournal := True; FNextBalance := 0; end;
                FResultNote.Caption := '暂时无法确认链上结果，正在自动重试。请勿重复付款。';
              end
              else if (Failure <> '') and BalanceJob then BalanceFailed(Failure)
              else if Failure <> '' then FStatus.Text := Failure + sLineBreak + '如已确认发送，请刷新本机记录并查询原交易，勿另建一笔付款。'
              else Done(Value);
            end;
          except on E: Exception do
            if FChainJob then begin FChainQueued := False; FChainInfo.Caption := '记录校验失败，请重新刷新'; end
            else if FTxJob or FSendJob then
            begin
              if FSendJob then FRecoverJournal := True;
              FResultNote.Caption := '结果暂未确认，正在自动重试。请勿重复付款。';
            end
            else if BalanceJob then BalanceFailed(E.Message) else FStatus.Text := E.Message;
          end;
          if BalanceJob then FNextBalance := GetTickCount64+1000;
          if FChainJob then
            if Failure<>'' then FNextHistory := GetTickCount64+15000
            else FNextHistory := GetTickCount64+5000;
          if FTxJob then FNextTransaction := GetTickCount64+2000;
          FTxJob := False; FSendJob := False;
          FBalanceJob := False; FChainJob := False; Wipe(Value); UpdateButtons;
        end);
    end);
  FWorker.FreeOnTerminate := False; FWorker.Start;
end;
procedure TTransferPanel.BalanceFailed(const Failure: string);
begin
  Inc(FBalanceFailures);
  FSync.Caption := '网络暂不可用，余额可能未更新';
  FSync.Visible := FBalanceFailures>=3;
  FSync.Hint := ''; FSync.ShowHint := False;
end;
procedure TTransferPanel.ApplyBalance(Balance: TJSONObject);
var Text, Usdt, Bnb: string;
  function DisplayAmount(const Value: string): string;
  begin
    Result := Value;
    if Pos('.',Result)>0 then
    begin
      while (Result<>'') and (Result[Length(Result)]='0') do Delete(Result,Length(Result),1);
      if (Result<>'') and (Result[Length(Result)]='.') then Delete(Result,Length(Result),1);
    end;
  end;
begin
  if (Balance.GetValue<Integer>('chain_id')<>56) or (Balance.GetValue<string>('asset')<>'BNB') or
    not SameText(Balance.GetValue<string>('address'),FAddress) then raise Exception.Create('余额响应与当前账户不一致');
  if (Balance.GetValue<string>('usdt_contract')<>UsdtContract) or (Balance.GetValue<Integer>('usdt_decimals')<>18) then raise Exception.Create('USDT 合约信息不一致');
  Text := 'USDT 余额：'+Balance.GetValue<string>('usdt_balance')+sLineBreak+
    'BNB 余额（支付手续费）：'+Balance.GetValue<string>('balance');
  if FBalance.Caption<>Text then FBalance.Caption := Text;
  Usdt := DisplayAmount(Balance.GetValue<string>('usdt_balance'));
  Bnb := DisplayAmount(Balance.GetValue<string>('balance'));
  if FHero.Caption<>Usdt then
  begin
    if Length(Usdt)>18 then FHero.Font.Size := 24 else FHero.Font.Size := 36;
    FHero.Caption := Usdt;
  end;
  FUsdtRow.Amount := Usdt; FBnbRow.Amount := Bnb;
  FUsdtRow.Hint := Usdt+' USDT'; FUsdtRow.ShowHint := True;
  FBnbRow.Hint := Bnb+' BNB'; FBnbRow.ShowHint := True;
  FLastBalance := FormatDateTime('hh:nn:ss',Now); FBalanceFailures := 0;
  FSync.Visible := False; FSync.Caption := ''; FSync.Hint := '';
end;
procedure TTransferPanel.BalanceTick(Sender: TObject);
var Account: UTF8String;
begin
  FNetwork.Enabled := not GetNavigationBusy and (FAddress<>'');
  if FNetworkIndex<>0 then Exit;
  if FPolling and not FBusy and not FSuspendAuto then
  begin
    if FRecoverJournal and (GetTickCount64>=FNextBalance) then begin RefreshAccount; Exit; end;
    if FReady and FUnresolved and (FHash<>'') and
      (FState<>'prepared') and (GetTickCount64>=FNextTransaction) then
    begin QueryTransaction; Exit; end;
  end;
  if not FSuspendAuto and FChainLoaded and (FCurrentPage=3) and not FBusy and not FChainQueued and
    (FChainEntries.Count<=3) and (GetTickCount64>=FNextHistory) then
  begin FChainAuto := True; FChainReadCache := False; FChainReset := True; FChainQueued := True; FChainScans := 0; end;
  if not FSuspendAuto and FChainQueued and not FBusy and (FAddress<>'') and (FCurrentPage=3) then begin LoadHistory; Exit; end;
  if not FPolling or FBusy or FSuspendAuto or (FCurrentPage=1) or ((FCurrentPage>=3) and (FCurrentPage<>6)) or (Assigned(FQuote) and (GetTickCount64<FQuoteExpiry)) or
    (FAddress='') or (GetTickCount64<FNextBalance) then Exit;
  if not FReady then begin RefreshAccount; Exit; end;
  Account := UTF8String(FAddress);
  RunJob(
    function: UTF8String begin Result := FApi.PublicRequest(5,[Account]); end,
    procedure(Value: UTF8String)
    var Data: TJSONObject;
    begin
      Data := JsonObject(Value);
      try ApplyBalance(Data); finally Data.Free; end;
    end,True);
end;
procedure TTransferPanel.RefreshAccount;
var Account: UTF8String; Epoch: UInt64;
begin
  Account := UTF8String(FAddress); Epoch := FApi.Epoch; FReady := False;
  RunJob(
    function: UTF8String
    var History, Balance: UTF8String; Data: TJSONObject;
    begin
      History := FApi.RequestAt(Epoch,10,[Account]);
      Balance := FApi.PublicRequest(5,[Account]);
      Data := TJSONObject.Create;
      try Data.AddPair('history',JsonObject(History)); Data.AddPair('balance',JsonObject(Balance)); Result := UTF8String(Data.ToJSON); finally Data.Free; end;
    end,
    procedure(Value: UTF8String)
    var Data, Balance, Item: TJSONObject; Entries: TJSONArray; I: Integer;
    begin
      Data := JsonObject(Value);
      try
        Balance := Data.GetValue<TJSONObject>('balance');
        ApplyBalance(Balance);
        FStatus.Lines.Clear;
        Entries := Data.GetValue<TJSONObject>('history').GetValue<TJSONArray>('entries');
        FId := ''; FHash := ''; FState := ''; FUnresolved := False;
        for I := 0 to Entries.Count-1 do
        begin
          Item := Entries.Items[I] as TJSONObject;
          if I=0 then begin FId := Item.GetValue<string>('id'); FHash := Item.GetValue<string>('hash'); FState := Item.GetValue<string>('state'); end;
          if (Item.GetValue<string>('state')<>'finalized') and (Item.GetValue<string>('state')<>'reverted') and (Item.GetValue<string>('state')<>'cancelled') then
          begin
            FUnresolved := True; FId := Item.GetValue<string>('id');
            FHash := Item.GetValue<string>('hash'); FState := Item.GetValue<string>('state');
          end;
          FStatus.Lines.Add(Item.GetValue<string>('summary').Replace(#10,' · ')+' · '+Item.GetValue<string>('state')+' · '+Item.GetValue<string>('hash'));
        end;
        FReady := True; FRecoverJournal := False; FNextTransaction := 0;
        if FHash<>'' then begin RenderTransactionResult; if FUnresolved and (FNetworkIndex=0) then ShowPage(6); end
        else if FCurrentPage=6 then
        begin FResultTitle.Caption := '未完成发送'; FResultNote.Caption := '未找到已签名交易，请在本机转账记录中查看并处理。'; end;
        if Entries.Count=0 then FStatus.Text := '暂无本机交易记录';
        if FUnresolved then FStatus.Lines.Add('存在未解决交易，已阻止新付款。正在自动查询原交易。');
      finally Data.Free; end;
    end,True);
end;
procedure TTransferPanel.PrepareTransfer;
var Account, Recipient, Amount: UTF8String; Asset: string; Opcode: Cardinal; MaxBnb: Boolean;
begin
  Asset := AssetName; if Asset='USDT' then Opcode := 6 else Opcode := 2;
  MaxBnb := FMaxBnb and (Asset='BNB');
  if MaxBnb then Opcode := 9;
  Account := UTF8String(FAddress); Recipient := UTF8String(FRecipient.Text);
  if not MaxBnb then Amount := UTF8String(CanonicalAmount(FAmount.Text));
  FreeAndNil(FQuote);
  RunJob(
    function: UTF8String
    begin
      if MaxBnb then Result := FApi.PublicRequest(Opcode,[Account,Recipient])
      else Result := FApi.PublicRequest(Opcode,[Account,Recipient,Amount]);
    end,
    procedure(Value: UTF8String)
    var Quote: TJSONObject; Price, Nonce, Gas: UInt64;
    begin
      Quote := JsonObject(Value);
      try
        if (Quote.GetValue<Integer>('chain_id')<>56) or (Quote.GetValue<string>('asset')<>Asset) or
          not SameText(Quote.GetValue<string>('sender'),string(Account)) or not SameText(Quote.GetValue<string>('recipient'),string(Recipient)) or
          (not MaxBnb and (Quote.GetValue<string>('amount')<>string(Amount))) then raise Exception.Create('转账预览与输入不一致');
        if not TryStrToUInt64(Quote.GetValue<string>('gas_limit'),Gas) then raise Exception.Create('Gas 格式错误');
        if (Asset='BNB') and (Gas<>21000) then raise Exception.Create('BNB Gas 不一致');
        if (Asset='USDT') and ((Quote.GetValue<string>('contract')<>UsdtContract) or (Quote.GetValue<Integer>('decimals')<>18)) then raise Exception.Create('USDT 合约不一致');
        if not TryStrToUInt64(Quote.GetValue<string>('gas_price_wei'),Price) or not TryStrToUInt64(Quote.GetValue<string>('nonce'),Nonce) then raise Exception.Create('费用或 nonce 格式错误');
        if Quote.GetValue<string>('maximum_fee')<>FeeText(Price,Gas) then raise Exception.Create('手续费计算不一致');
        if MaxBnb then
        begin
          if CanonicalAmount(Quote.GetValue<string>('amount'))='0.000000000000000000' then
            raise Exception.Create('扣除手续费后没有可发送的 BNB');
          FAmount.Text := CanonicalAmount(Quote.GetValue<string>('amount'));
          FMaxBnb := True;
          FStatus.Text := '已扣除本次网络手续费，核对后点击“确认并发送 BNB”。费用以 BNB 计价。';
        end;
        FPreview.Text := 'BSC 主网 / '+Asset+sLineBreak+'从：'+Quote.GetValue<string>('sender')+sLineBreak+'到：'+Quote.GetValue<string>('recipient')+sLineBreak+
          '数量：'+Quote.GetValue<string>('amount')+' '+Asset+sLineBreak+'手续费上限：'+Quote.GetValue<string>('maximum_fee')+' BNB'+sLineBreak+'nonce：'+UIntToStr(Nonce);
        if Asset='USDT' then FPreview.Lines.Add('USDT 合约：'+UsdtContract);
        FQuote := Quote; Quote := nil; FQuoteExpiry := GetTickCount64+30000;
      finally Quote.Free; end;
    end);
end;
procedure TTransferPanel.SendTransfer(Replay: Boolean);
var Profile, Account, Recipient, Amount, Nonce, Price, Gas, Id, Hash, Expected: UTF8String;
    Epoch: UInt64; Generation: Integer; Prompt, Asset: string;
begin
  if SameText(ParamStr(1),'--public-preview') then raise Exception.Create('公开预览禁止签名或广播');
  if not FReady then Exit;
  if not Replay and (not Assigned(FQuote) or (GetTickCount64>=FQuoteExpiry)) then raise Exception.Create('预览已过期，请重新获取手续费');
  Profile := UTF8String(FProfile); Account := UTF8String(FAddress); Epoch := FApi.Epoch; Generation := FGeneration;
  if Replay then begin Id := UTF8String(FId); Hash := UTF8String(FHash); Prompt := '只重播原交易，不生成新付款：'+sLineBreak+FHash; end
  else begin
    Asset := FQuote.GetValue<string>('asset'); Gas := UTF8String(FQuote.GetValue<string>('gas_limit'));
    Recipient := UTF8String(FQuote.GetValue<string>('recipient')); Amount := UTF8String(FQuote.GetValue<string>('amount'));
    Nonce := UTF8String(FQuote.GetValue<string>('nonce')); Price := UTF8String(FQuote.GetValue<string>('gas_price_wei'));
    Expected := UTF8String(Asset)+#10+'56'+#10+Account+#10+Recipient+#10+Amount+#10+Price+#10+Gas+#10+Nonce;
    if Asset='USDT' then Expected := Expected+#10+UTF8String(UsdtContract);
    Prompt := FPreview.Text+sLineBreak+sLineBreak+'确认发送真实 '+Asset+'，并使用 BNB 支付手续费？';
  end;
  if MessageDlg(Prompt,mtConfirmation,[mbYes,mbNo],0)<>mrYes then Exit;
  if (Generation<>FGeneration) or (not Replay and (GetTickCount64>=FQuoteExpiry)) then begin raise Exception.Create('会话或预览已失效'); end;
  FreeAndNil(FQuote); FUnresolved := True;
  FSendJob := True; FState := 'broadcasting';
  if not Replay then begin FId := ''; FHash := ''; end;
  RenderTransactionResult; FResultNote.Caption := '正在提交交易，请稍候…'; ShowPage(6);
  RunJob(
    function: UTF8String
    var Receipt, Raw, Response: UTF8String; Lines: TArray<string>; I: Integer; Summary: UTF8String; Data: TJSONObject;
    begin
      try
        if not Replay then
        begin
          if Asset='USDT' then Receipt := FApi.RequestAt(Epoch,16,[Profile,Account,Recipient,Amount,Nonce,Price,Gas])
          else Receipt := FApi.RequestAt(Epoch,15,[Profile,Account,Recipient,Amount,Nonce,Price]);
          Lines := string(Receipt).Split([#10]);
          if ((Asset='USDT') and (Length(Lines)<>11)) or ((Asset='BNB') and (Length(Lines)<>10)) then raise Exception.Create('签名回执结构错误，已停止广播');
          Id := UTF8String(Lines[0]); Hash := UTF8String(Lines[1]); Summary := '';
          for I := 2 to High(Lines) do begin if I>2 then Summary := Summary+#10; Summary := Summary+UTF8String(Lines[I]); end;
          if Summary<>Expected then raise Exception.Create('签名内容与确认内容不一致，已停止广播');
        end;
        Raw := FApi.RequestAt(Epoch,17,[Profile,Id]);
        if TInterlocked.CompareExchange(FGeneration,0,0)<>Generation then raise Exception.Create('已锁定，停止新的广播请求');
        Response := FApi.PublicRequest(3,[HexBytes(Raw),Hash]);
        Data := JsonObject(Response);
        try if Data.GetValue<string>('hash')<>string(Hash) then raise Exception.Create('广播哈希不一致'); finally Data.Free; end;
        Result := '广播请求已受理，尚未最终确认。请查询原交易：'+#10+Hash;
      finally Wipe(Raw); Wipe(Receipt); end;
    end,
    procedure(Value: UTF8String)
    begin FId := string(Id); FHash := string(Hash); FState := 'broadcasting'; FStatus.Text := string(Value); FNextTransaction := 0; RenderTransactionResult; end);
end;
procedure TTransferPanel.QueryTransaction;
var Id, Hash: UTF8String; Epoch: UInt64;
begin
  if FBusy or (FHash='') then Exit;
  Id := UTF8String(FId); Hash := UTF8String(FHash); Epoch := FApi.Epoch; FTxJob := True;
  RunJob(
    function: UTF8String
    var Data: TJSONObject; State, Ignore: UTF8String;
    begin
      Result := FApi.PublicRequest(4,[Hash]); Data := JsonObject(Result);
      try
        if Data.GetValue<string>('hash')<>string(Hash) then raise Exception.Create('回执哈希不一致');
        State := UTF8String(Data.GetValue<string>('state'));
        if (State='included') or (State='finalized') or (State='reverted') then
          Ignore := FApi.RequestAt(Epoch,11,[Id,Hash,State,UTF8String(Data.GetValue<string>('block_number')),UTF8String(Data.GetValue<string>('block_hash')),UTF8String(Data.GetValue<string>('finalized_height'))]);
      finally Data.Free; end;
    end,
    procedure(Value: UTF8String)
    var Data: TJSONObject;
    begin
      Data := JsonObject(Value);
      try
        FState := Data.GetValue<string>('state');
        FUnresolved := (FState<>'finalized') and (FState<>'reverted');
        RenderTransactionResult;
        if not FUnresolved then
        begin
          FNextBalance := 0; FNextHistory := 0; FChainReadCache := False;
        end;
        if FState='finalized' then FStatus.Text := '链上成功，已达到节点报告的 finalized 状态。'+sLineBreak+FHash
        else if FState='reverted' then FStatus.Text := '链上执行失败，nonce 已消耗，仍会产生手续费。'+sLineBreak+FHash
        else if FState='included' then FStatus.Text := '已入块，等待最终确认。'+sLineBreak+FHash
        else FStatus.Text := '结果未知。保留原哈希，不生成替代付款；稍后继续查询。'+sLineBreak+FHash;
      finally Data.Free; end;
    end);
end;
procedure TTransferPanel.CancelPrepared;
var Id: UTF8String; Epoch: UInt64;
begin
  if FState<>'prepared' then Exit;
  if MessageDlg('只取消尚未保存签名、从未向网络释放载荷的意图。确认取消？',mtConfirmation,[mbYes,mbNo],0)<>mrYes then Exit;
  Id := UTF8String(FId); Epoch := FApi.Epoch;
  RunJob(function: UTF8String begin Result := FApi.RequestAt(Epoch,12,[Id]); end,
    procedure(Value: UTF8String) begin FState := 'cancelled'; FUnresolved := False; FStatus.Text := '未签名意图已取消。'; end);
end;
procedure TTransferPanel.Action(Sender: TObject);
begin
  if FBusy then Exit;
  FSuspendAuto := True;
  try
    try
    case TButton(Sender).Tag of
      1: RefreshAccount; 2: PrepareTransfer; 3: SendTransfer(False);
      4: begin RenderTransactionResult; ShowPage(6); QueryTransaction; end; 5: SendTransfer(True); 6: CancelPrepared;
    end;
    except on E: Exception do FStatus.Text := E.Message; end;
  finally FSuspendAuto := False; end;
end;
procedure TTransferPanel.RunReadOnlySelfTest(Api: TSecureWalletApi);
var Deadline: UInt64; SavedBalance: string; Worker: TThread; Item: TJSONObject; I: Integer;
  procedure WaitForJob;
  begin
    Deadline := GetTickCount64+55000;
    while FBusy and (GetTickCount64<Deadline) do begin Application.ProcessMessages; Sleep(5); end;
    if FBusy then raise Exception.Create('Async public test timed out');
  end;
begin
  SetAccount(Api,'public-readonly','0x0000000000000000000000000000000000000000');
  WaitForJob;
  if not FReady or FSend.Enabled then raise Exception.Create('Async balance/empty preview state failed: '+FStatus.Text);
  if (AssetName<>'USDT') or (Pos('USDT 余额',FBalance.Caption)=0) or (Pos('BNB 余额',FBalance.Caption)=0) then raise Exception.Create('USDT default or dual balances missing');
  ShowPage(1);
  if FHome.Visible or not FSendPage.Visible then raise Exception.Create('Send navigation failed');
  ShowPage(2);
  if not FReceivePage.Visible or (FReceiveAddress.Caption<>FAddress) then raise Exception.Create('Receive navigation/address failed');
  ShowPage(3);
  if not FHistoryPage.Visible then raise Exception.Create('Chain history navigation failed');
  ShowPage(4);
  if not FLocalPage.Visible then raise Exception.Create('Local history navigation/password failed');
  // The BNB history path must work through the offline signer, without any network history request.
  FSuspendAuto := True;
  FHistoryAsset.ItemIndex := 1; HistoryAssetChanged(nil); ShowPage(3);
  LoadHistory; WaitForJob;
  if not FChainLoaded or (FChainEntries.Count<>0) or FChainQueued or (FChainCursor<>'') then
    raise Exception.Create('Offline BNB history failed: '+FChainInfo.Caption);
  FHistoryAsset.ItemIndex := 0; HistoryAssetChanged(nil); FChainQueued := False;
  FSuspendAuto := False;
  // Public synthetic records exercise date grouping and detail selection, never the payment journal.
  for I := 0 to 2 do
  begin
    Item := TJSONObject.Create;
    Item.AddPair('h','0x'+StringOfChar(Char(Ord('a')+I),64)); Item.AddPair('i',TJSONNumber.Create(I));
    Item.AddPair('b',TJSONNumber.Create(100000-I)); Item.AddPair('t',TJSONNumber.Create(Int64(1700000000)-(I div 2)*86400));
    if I=1 then begin Item.AddPair('from',FAddress); Item.AddPair('to','0x1111111111111111111111111111111111111111'); end
    else begin Item.AddPair('from','0x1111111111111111111111111111111111111111'); Item.AddPair('to',FAddress); end;
    Item.AddPair('a','10.000000000000000000'); FChainEntries.AddElement(Item);
  end;
  RenderHistory; ShowPage(3);
  if FChainList.ControlCount<>5 then raise Exception.Create('History date grouping failed');
  HistoryDetail(FChainRefresh);
  if (FCurrentPage<>5) or (FSelectedHash<>'0x'+StringOfChar('a',64)) or (Pos('接收 USDT',FDetailText.Text)=0) then
    raise Exception.Create('Incoming history detail failed');
  FHash := StringOfChar('a',64); FState := 'broadcasting'; RenderTransactionResult; ShowPage(6);
  if not FResultPage.Visible or (FResultTitle.Caption<>'转账确认中') then raise Exception.Create('Pending result missing');
  FState := 'included'; RenderTransactionResult;
  if FResultTitle.Caption='转账成功' then raise Exception.Create('Premature success');
  FState := 'finalized'; RenderTransactionResult;
  if FResultTitle.Caption<>'转账成功' then raise Exception.Create('Final result missing');
  FState := 'reverted'; RenderTransactionResult;
  if FResultTitle.Caption<>'转账失败' then raise Exception.Create('Reverted result missing');
  FTxJob := True;
  RunJob(function: UTF8String begin raise Exception.Create('TEST RECEIPT OFFLINE'); end,
    procedure(Value: UTF8String) begin raise Exception.Create('Unexpected receipt success'); end);
  WaitForJob;
  if (FState<>'reverted') or (FNextTransaction<=GetTickCount64) or FTxJob then
    raise Exception.Create('Receipt retry scheduling failed');
  FHash := ''; FState := ''; ShowPage(0);
  FStatus.Text := 'TEST TRANSACTION RESULT'; FAmount.Text := '1.25';
  SavedBalance := FBalance.Caption;
  RunJob(function: UTF8String begin raise Exception.Create('TEST OFFLINE'); end,
    procedure(Value: UTF8String) begin raise Exception.Create('Unexpected success'); end,True);
  WaitForJob;
  if FSync.Visible then raise Exception.Create('Transient failure should remain quiet');
  BalanceFailed('TEST OFFLINE'); BalanceFailed('TEST OFFLINE');
  if (FBalance.Caption<>SavedBalance) or not FSync.Visible or
    (Trim(FStatus.Text)<>'TEST TRANSACTION RESULT') then raise Exception.Create('Stale balance preservation failed');
  FNextBalance := 0; FSuspendAuto := True; BalanceTick(nil);
  if FBusy then raise Exception.Create('Polling during confirmation');
  FSuspendAuto := False;
  FQuote := TJSONObject.Create; FQuoteExpiry := GetTickCount64+30000; BalanceTick(nil);
  if FBusy then raise Exception.Create('Polling interrupted live quote');
  FreeAndNil(FQuote);
  BalanceTick(nil); Worker := FWorker;
  if not FBusy or not FAmount.Enabled then raise Exception.Create('Automatic balance refresh did not start');
  BalanceTick(nil);
  if FWorker<>Worker then raise Exception.Create('Overlapping balance requests');
  WaitForJob;
  if FSync.Visible or (Trim(FStatus.Text)<>'TEST TRANSACTION RESULT') or
    (FAmount.Text<>'1.25') then raise Exception.Create('Automatic recovery/input preservation failed');
  FNextBalance := 0; BalanceTick(nil);
  ClearAccount; WaitForJob;
  if (FBalance.Caption<>'余额尚未查询') or (FAddress<>'') then raise Exception.Create('Late balance repopulated locked account');
  FNextBalance := 0; BalanceTick(nil);
  if FBusy then raise Exception.Create('Polling continued after lock');
  FAsset.ItemIndex := 1; Changed(FAsset);
  if (FSend.Caption<>'确认并发送 BNB') or Assigned(FQuote) then raise Exception.Create('BNB asset switch failed');
  FAsset.ItemIndex := 0; Changed(FAsset);
  Lock;
  if FSend.Enabled then raise Exception.Create('Transfer lock clearing failed');
end;
procedure TTransferPanel.RunNetworkNavigationSelfTest;
var Index, Ppi: Integer; Completed: Boolean; Deadline: UInt64;
begin
  FPreviewMode := True; FAddress := '0x0000000000000000000000000000000000000000'; FPolling := False;
  try
    for Ppi in [96,144,96] do
    begin
      ScaleForPPI(Ppi); Width := MulDiv(540,Ppi,96); Height := MulDiv(800,Ppi,96);
      for Index in [1,2,0,2,1,0] do
      begin
        FNetwork.ItemIndex := Index; NetworkChanged(nil);
        if FNetworkIndex<>Index then raise Exception.Create('Network navigation failed');
        if (Index<>0) and (not FNetworkHost.Visible or FHome.Visible) then raise Exception.Create('Mixed-chain pages visible');
        if (Index=1) and ((FSolana.Parent<>FNetworkHost) or (FSolana.BorderStyle<>bsNone)) then raise Exception.Create('Solana is not embedded');
        if (Index=2) and ((FTron.Parent<>FNetworkHost) or (FTron.BorderStyle<>bsNone)) then raise Exception.Create('Tron is not embedded');
        ShowPage(1); ShowPage(2); ShowPage(3); ShowPage(0);
      end;
      FBusy := True; FNetwork.ItemIndex := 2; NetworkChanged(nil); FBusy := False;
      if (FNetworkIndex<>0) or (FNetwork.ItemIndex<>0) then raise Exception.Create('Busy operation allowed chain switch');
    end;
    // Keep a real worker outstanding while exercising the selector and timer.
    Completed := False;
    RunJob(function: UTF8String begin Sleep(150); Result := ''; end,
      procedure(Value: UTF8String) begin Completed := True; end,True);
    FNetwork.DroppedDown := True;
    BalanceTick(nil);
    if not FNetwork.Enabled or not FNetwork.DroppedDown then
      raise Exception.Create('Balance refresh interrupted open network selector');
    FNetwork.DroppedDown := False;
    for Index in [2,1,0,2] do
    begin
      FNetwork.ItemIndex := Index; NetworkChanged(nil); BalanceTick(nil);
      if (FNetworkIndex<>Index) or not FNetwork.Enabled then
        raise Exception.Create('Balance refresh prevented network selection');
    end;
    ShowPage(2); FNetwork.DroppedDown := True;
    Deadline := GetTickCount64+5000;
    while FBusy and (GetTickCount64<Deadline) do begin Application.ProcessMessages; Sleep(5); end;
    if FBusy or not Completed or (FNetworkIndex<>2) or not FNetworkHost.Visible or
      not FNetwork.Enabled or not FNetwork.DroppedDown then
      raise Exception.CreateFmt('Background completion disturbed selected network: busy=%s done=%s network=%d visible=%s enabled=%s dropdown=%s',[BoolToStr(FBusy,True),BoolToStr(Completed,True),FNetworkIndex,BoolToStr(FNetworkHost.Visible,True),BoolToStr(FNetwork.Enabled,True),BoolToStr(FNetwork.DroppedDown,True)]);
    FNetwork.DroppedDown := False;
    FNetwork.ItemIndex := 2; NetworkChanged(nil); Lock;
    if FNetworkHost.Visible or (FNetwork.ItemIndex<>0) then raise Exception.Create('Lock retained chain view');
  finally
    FreeAndNil(FSolana); FreeAndNil(FTron); FPreviewMode := False; ClearAccount; ShowPage(0);
  end;
end;
procedure TTransferPanel.CheckLayout;
begin
  ShowPage(1);
  if (FSend.Left+FSend.Width>FSendPage.ClientWidth) or (FSend.Top+FSend.Height>FSendPage.ClientHeight) then
    raise Exception.CreateFmt('Send layout clipped: ppi=%d page=%dx%d button=%d,%d',[CurrentPPI,FSendPage.ClientWidth,FSendPage.ClientHeight,FSend.Left+FSend.Width,FSend.Top+FSend.Height]);
  ShowPage(4);
  if (FCancel.Left+FCancel.Width>FLocalPage.ClientWidth) or (FCancel.Top+FCancel.Height>FLocalPage.ClientHeight) then
    raise Exception.Create('History layout clipped');
  ShowPage(3);
  if (FExplorer.Left+FExplorer.Width>FHistoryPage.ClientWidth) or
    (FExplorer.Top+FExplorer.Height>FHistoryPage.ClientHeight) then
    raise Exception.Create('Explorer button clipped');
  ShowPage(0);
end;
end.
